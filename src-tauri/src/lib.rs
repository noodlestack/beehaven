pub mod agents;
pub mod demo;
pub mod diagnostics;
pub mod error;
pub mod git;
pub mod models;
pub mod queue;
pub mod runner;
pub mod session;
pub mod store;
pub mod time_util;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::time_util::{now_compact_utc, now_rfc3339};

use crate::agents::{
    detect_all_agents, get_adapter, start_hook_listener, AgentSignal, HookMessage,
};
use crate::diagnostics::{export_diagnostics_bundle, init_logging};
use crate::error::{AppError, AppErrorPayload};
use crate::git::conflict::{
    forecast_pair, suggest_merge_order, CandidateBranchMetric, MergeTreeOutcome,
};
use crate::git::diff::{diff_summary, file_diff};
use crate::git::safety::{normalize_existing_path, run_git};
use crate::git::worktree::{
    create_worktree, default_worktree_root, inspect_repo, list_worktrees, remove_worktree,
    save_backup_patch,
};
use crate::models::{
    AgentInfo, AppSettings, ConflictCell, ConflictMatrixReport, ConflictStatus, DiffSummary,
    FileDiff, QueueState, Repo, RepoCheckResult, SessionOutputPayload, SessionRecord, Task,
    TaskEvent, TaskState, TaskStatePayload, TestRun, TestsProgressPayload, WorktreeInfo,
};
use crate::queue::MergeQueueController;
use crate::runner::run_test_command;
use crate::session::{SessionCallbackEvent, SessionManager};
use crate::store::Store;

pub struct AppCore {
    pub store: Arc<Store>,
    pub sessions: SessionManager,
    pub queue: MergeQueueController,
    pub data_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub hook_port: u16,
    pub hook_token: String,
    pub pty_event_tx: mpsc::UnboundedSender<SessionCallbackEvent>,
    _log_guard: Mutex<Option<tracing_appender::non_blocking::WorkerGuard>>,
}

type CmdResult<T> = Result<T, AppErrorPayload>;

fn map_err(e: AppError) -> AppErrorPayload {
    tracing::warn!(code = %e.to_payload().code, "Command error: {}", e);
    e.to_payload()
}

impl AppCore {
    /// Checks concurrency slots and starts queued tasks up to `max_concurrent_agents`.
    pub fn schedule_queued_tasks(&self, app: &AppHandle, repo_id: &str) {
        let settings = self.store.get_settings().unwrap_or_default();
        let max_slots = settings.max_concurrent_agents.max(1);
        let Ok(tasks) = self.store.list_tasks(repo_id) else {
            return;
        };

        let mut running_count = tasks
            .iter()
            .filter(|t| matches!(t.state, TaskState::Running | TaskState::WaitingForYou))
            .count();

        for task in tasks.into_iter().filter(|t| t.state == TaskState::Queued) {
            if running_count >= max_slots {
                break;
            }
            if self.launch_task_internal(app, &task.id, None).is_ok() {
                running_count += 1;
            }
        }
    }

    pub fn launch_task_internal(
        &self,
        app: &AppHandle,
        task_id: &str,
        revision_prompt: Option<String>,
    ) -> Result<Task, AppError> {
        let mut task = self.store.get_task(task_id)?;
        if let Some(extra) = revision_prompt.as_ref().filter(|s| !s.trim().is_empty()) {
            task.prompt = extra.clone();
        }

        let adapter = get_adapter(&task.agent);
        let wt_path = Path::new(&task.worktree_path);
        adapter.prepare(wt_path, &task, self.hook_port, &self.hook_token)?;

        let spec = adapter.command(&task, self.hook_port, &self.hook_token);
        let settings = self.store.get_settings()?;
        let silence_opt = if adapter.supports_hooks() {
            None
        } else {
            Some(settings.silence_timeout_secs)
        };

        let session_id = uuid::Uuid::new_v4().to_string();
        let pid = self.sessions.spawn_session(
            task.id.clone(),
            session_id.clone(),
            spec,
            110,
            32,
            silence_opt,
            self.pty_event_tx.clone(),
        )?;

        self.store.record_session_start(&SessionRecord {
            id: session_id,
            task_id: task.id.clone(),
            pid,
            started_at: now_rfc3339(),
            ended_at: None,
            exit_code: None,
        })?;

        // For interactive shell adapter, display helpful prompt banner right inside the shell
        if task.agent == "generic" && !task.prompt.trim().is_empty() {
            let safe_banner = task.prompt.replace(['\r', '\n'], " ").replace('"', "'");
            let _ = self.sessions.write_input(
                &task.id,
                &format!("echo \"[Beehaven Task] {safe_banner}\"\r\n"),
            );
        }

        let updated = self.store.transition_task_state(
            &task.id,
            TaskState::Running,
            "Agent session started",
            None,
            None,
        )?;
        let _ = app.emit(
            "task:state",
            TaskStatePayload {
                task: updated.clone(),
                reason: "Agent session started".to_string(),
            },
        );
        Ok(updated)
    }
}

// ============================================================================
// Typed Tauri IPC Commands (Section 4.6)
// ============================================================================

#[tauri::command]
fn repo_check(path: String) -> CmdResult<RepoCheckResult> {
    let agents = detect_all_agents();
    inspect_repo(Path::new(&path), agents).map_err(map_err)
}

#[tauri::command]
fn repo_open(
    state: State<'_, Arc<AppCore>>,
    path: String,
    base_branch: Option<String>,
    test_command: Option<String>,
) -> CmdResult<Repo> {
    let agents = detect_all_agents();
    let check = inspect_repo(Path::new(&path), agents).map_err(map_err)?;
    if !check.git_version_ok {
        return Err(AppError::user(
            "GIT_TOO_OLD",
            format!(
                "Beehaven requires Git 2.38 or newer for conflict forecasting (found '{}').",
                check.git_version
            ),
        )
        .to_payload());
    }
    if !check.is_git_repo {
        return Err(AppError::user(
            "NOT_A_GIT_REPO",
            "The selected folder is not a Git repository. Run `git init` and make an initial commit first.",
        )
        .to_payload());
    }

    let canon = normalize_existing_path(Path::new(&check.path)).map_err(map_err)?;
    let wt_root = default_worktree_root(&canon).map_err(map_err)?;
    let name = canon
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repository")
        .to_string();

    let repo = Repo {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: canon.to_string_lossy().to_string(),
        base_branch: base_branch
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(check.detected_base_branch),
        test_command: test_command
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(check.suggested_test_command),
        worktree_root: wt_root.to_string_lossy().to_string(),
        created_at: now_rfc3339(),
    };
    state.store.upsert_repo(&repo).map_err(map_err)
}

#[tauri::command]
fn repo_list(state: State<'_, Arc<AppCore>>) -> CmdResult<Vec<Repo>> {
    state.store.list_repos().map_err(map_err)
}

#[tauri::command]
async fn repo_create_demo(state: State<'_, Arc<AppCore>>) -> CmdResult<Repo> {
    crate::demo::create_demo_sandbox(&state.data_dir, &state.logs_dir, &state.store)
        .await
        .map_err(map_err)
}

#[tauri::command]
fn worktree_list(state: State<'_, Arc<AppCore>>, repo_id: String) -> CmdResult<Vec<WorktreeInfo>> {
    let repo = state.store.get_repo(&repo_id).map_err(map_err)?;
    list_worktrees(Path::new(&repo.path)).map_err(map_err)
}

#[tauri::command]
fn agents_list() -> CmdResult<Vec<AgentInfo>> {
    Ok(detect_all_agents())
}

#[tauri::command]
fn task_list(state: State<'_, Arc<AppCore>>, repo_id: String) -> CmdResult<Vec<Task>> {
    state.store.list_tasks(&repo_id).map_err(map_err)
}

#[tauri::command]
fn task_create(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    repo_id: String,
    title: String,
    prompt: String,
    agent: Option<String>,
    auto_start: Option<bool>,
) -> CmdResult<Task> {
    if title.trim().is_empty() {
        return Err(
            AppError::user("EMPTY_TITLE", "Enter a short title for the task.").to_payload(),
        );
    }
    let repo = state.store.get_repo(&repo_id).map_err(map_err)?;
    let settings = state.store.get_settings().map_err(map_err)?;
    let chosen_agent = agent
        .filter(|a| !a.trim().is_empty())
        .unwrap_or(settings.default_agent);

    let task_id = uuid::Uuid::new_v4().to_string();
    let (branch, wt_path) = create_worktree(
        Path::new(&repo.path),
        Path::new(&repo.worktree_root),
        &repo.base_branch,
        &task_id,
        &title,
    )
    .map_err(map_err)?;

    let now = now_rfc3339();
    let task = Task {
        id: task_id.clone(),
        repo_id: repo.id.clone(),
        title: title.trim().to_string(),
        prompt,
        branch,
        worktree_path: wt_path.to_string_lossy().to_string(),
        agent: chosen_agent,
        state: TaskState::Queued,
        backup_patch_path: None,
        last_error: None,
        created_at: now.clone(),
        updated_at: now,
    };
    state.store.insert_task(&task).map_err(map_err)?;
    let _ = app.emit(
        "task:state",
        TaskStatePayload {
            task: task.clone(),
            reason: "Task created".to_string(),
        },
    );

    if auto_start.unwrap_or(true) {
        state.schedule_queued_tasks(&app, &repo_id);
    }

    state.store.get_task(&task_id).map_err(map_err)
}

#[tauri::command]
fn task_start(app: AppHandle, state: State<'_, Arc<AppCore>>, task_id: String) -> CmdResult<Task> {
    state
        .launch_task_internal(&app, &task_id, None)
        .map_err(map_err)
}

#[tauri::command]
fn task_set_manual_state(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    target_state: TaskState,
) -> CmdResult<Task> {
    let updated = state
        .store
        .transition_task_state(
            &task_id,
            target_state,
            "Updated manually by user",
            None,
            None,
        )
        .map_err(map_err)?;
    let _ = app.emit(
        "task:state",
        TaskStatePayload {
            task: updated.clone(),
            reason: "Updated manually by user".to_string(),
        },
    );
    state.schedule_queued_tasks(&app, &updated.repo_id);
    Ok(updated)
}

#[tauri::command]
fn task_approve(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
) -> CmdResult<Task> {
    let updated = state
        .store
        .transition_task_state(
            &task_id,
            TaskState::Approved,
            "Approved for merge queue",
            None,
            None,
        )
        .map_err(map_err)?;
    let _ = app.emit(
        "task:state",
        TaskStatePayload {
            task: updated.clone(),
            reason: "Approved for merge queue".to_string(),
        },
    );
    Ok(updated)
}

#[tauri::command]
fn task_revise(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    message: String,
) -> CmdResult<Task> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    if state.sessions.is_active(&task_id) {
        state
            .sessions
            .write_input(&task_id, &format!("{message}\r\n"))
            .map_err(map_err)?;
        let updated = state
            .store
            .transition_task_state(
                &task_id,
                TaskState::Running,
                "Sent revision prompt to active terminal",
                None,
                None,
            )
            .map_err(map_err)?;
        let _ = app.emit(
            "task:state",
            TaskStatePayload {
                task: updated.clone(),
                reason: "Revision prompt sent".to_string(),
            },
        );
        return Ok(updated);
    }

    state
        .launch_task_internal(&app, &task.id, Some(message))
        .map_err(map_err)
}

#[tauri::command]
fn task_discard(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    force: bool,
) -> CmdResult<Task> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let repo = state.store.get_repo(&task.repo_id).map_err(map_err)?;

    // Stop any running PTY tree first
    let _ = state.sessions.kill_session(&task_id);

    let repo_backup_dir = state.backups_dir.join(&repo.id);
    let saved_patch = remove_worktree(
        Path::new(&repo.path),
        Path::new(&repo.worktree_root),
        Path::new(&task.worktree_path),
        Some(&task.branch),
        Some(&repo.base_branch),
        force,
        &repo_backup_dir,
    )
    .map_err(map_err)?;

    let patch_str = saved_patch.map(|p| p.to_string_lossy().to_string());
    let updated = state
        .store
        .transition_task_state(
            &task_id,
            TaskState::Discarded,
            "Task discarded and backup patch saved",
            None,
            patch_str,
        )
        .map_err(map_err)?;

    let _ = app.emit(
        "task:state",
        TaskStatePayload {
            task: updated.clone(),
            reason: "Task discarded (backup patch saved)".to_string(),
        },
    );
    state.schedule_queued_tasks(&app, &repo.id);
    Ok(updated)
}

#[tauri::command]
fn session_replay(state: State<'_, Arc<AppCore>>, task_id: String) -> CmdResult<String> {
    Ok(state.sessions.replay_output(&task_id))
}

#[tauri::command]
fn session_write(state: State<'_, Arc<AppCore>>, task_id: String, data: String) -> CmdResult<()> {
    state.sessions.write_input(&task_id, &data).map_err(map_err)
}

#[tauri::command]
fn session_resize(
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    cols: u16,
    rows: u16,
) -> CmdResult<()> {
    state.sessions.resize(&task_id, cols, rows).map_err(map_err)
}

#[tauri::command]
fn session_kill(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
) -> CmdResult<Task> {
    state.sessions.kill_session(&task_id).map_err(map_err)?;
    let current = state.store.get_task(&task_id).map_err(map_err)?;
    let updated = if matches!(current.state, TaskState::Running | TaskState::WaitingForYou) {
        let u = state
            .store
            .transition_task_state(
                &task_id,
                TaskState::NeedsReview,
                "Terminal session stopped by user",
                None,
                None,
            )
            .map_err(map_err)?;
        let _ = app.emit(
            "task:state",
            TaskStatePayload {
                task: u.clone(),
                reason: "Session stopped".to_string(),
            },
        );
        u
    } else {
        current
    };
    state.schedule_queued_tasks(&app, &updated.repo_id);
    Ok(updated)
}

#[tauri::command]
fn diff_summary_get(state: State<'_, Arc<AppCore>>, task_id: String) -> CmdResult<DiffSummary> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let repo = state.store.get_repo(&task.repo_id).map_err(map_err)?;
    diff_summary(&task.id, Path::new(&task.worktree_path), &repo.base_branch).map_err(map_err)
}

#[tauri::command]
fn diff_file_get(
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    file_path: String,
) -> CmdResult<FileDiff> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let repo = state.store.get_repo(&task.repo_id).map_err(map_err)?;
    file_diff(
        Path::new(&task.worktree_path),
        &repo.base_branch,
        &file_path,
    )
    .map_err(map_err)
}

#[tauri::command]
async fn tests_run(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    task_id: String,
) -> CmdResult<TestRun> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let repo = state.store.get_repo(&task.repo_id).map_err(map_err)?;
    let app_stream = app.clone();
    let tid_stream = task.id.clone();

    let run = run_test_command(
        &task.id,
        Path::new(&task.worktree_path),
        &repo.test_command,
        &state.logs_dir,
        Duration::from_secs(300),
        move |chunk| {
            let _ = app_stream.emit(
                "tests:progress",
                TestsProgressPayload {
                    task_id: tid_stream.clone(),
                    chunk,
                },
            );
        },
    )
    .await
    .map_err(map_err)?;

    state.store.insert_test_run(&run).map_err(map_err)?;
    let _ = app.emit("tests:done", run.clone());
    Ok(run)
}

#[tauri::command]
fn tests_list(state: State<'_, Arc<AppCore>>, task_id: String) -> CmdResult<Vec<TestRun>> {
    state.store.list_test_runs(&task_id).map_err(map_err)
}

#[tauri::command]
async fn conflicts_compute(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    repo_id: String,
) -> CmdResult<ConflictMatrixReport> {
    let repo = state.store.get_repo(&repo_id).map_err(map_err)?;
    let all_tasks = state.store.list_tasks(&repo_id).map_err(map_err)?;

    // Active unmerged task branches
    let active: Vec<Task> = all_tasks
        .into_iter()
        .filter(|t| !matches!(t.state, TaskState::Merged | TaskState::Discarded))
        .filter(|t| Path::new(&t.worktree_path).exists())
        .collect();

    let repo_path = PathBuf::from(&repo.path);
    let mut sha_by_task = std::collections::HashMap::<String, String>::new();
    let mut metrics = Vec::<CandidateBranchMetric>::new();

    for (idx, t) in active.iter().enumerate() {
        let rev = run_git(&repo_path, &["rev-parse", &t.branch]).map_err(map_err)?;
        if !rev.status.success() {
            continue;
        }
        let sha = String::from_utf8_lossy(&rev.stdout).trim().to_string();
        sha_by_task.insert(t.id.clone(), sha);

        let lines = diff_summary(&t.id, Path::new(&t.worktree_path), &repo.base_branch)
            .map(|d| d.total_additions + d.total_deletions)
            .unwrap_or(0);
        metrics.push(CandidateBranchMetric {
            task_id: t.id.clone(),
            index_label: format!("{:04}", idx + 1),
            title: t.title.clone(),
            changed_lines: lines,
        });
    }

    let mut pairs_to_compute = Vec::new();
    let mut cells = Vec::<ConflictCell>::new();

    for i in 0..active.len() {
        for j in (i + 1)..active.len() {
            let ta = &active[i];
            let tb = &active[j];
            let (Some(sha_a), Some(sha_b)) = (sha_by_task.get(&ta.id), sha_by_task.get(&tb.id))
            else {
                continue;
            };

            if let Ok(Some(cached)) = state.store.get_cached_conflict(&repo.id, sha_a, sha_b) {
                cells.push(ConflictCell {
                    task_a_id: ta.id.clone(),
                    task_b_id: tb.id.clone(),
                    sha_a: sha_a.clone(),
                    sha_b: sha_b.clone(),
                    status: cached.status,
                    files: cached.files,
                    error_message: cached.error_message,
                    computed_at: cached.computed_at,
                });
            } else {
                pairs_to_compute.push((
                    ta.id.clone(),
                    tb.id.clone(),
                    ta.branch.clone(),
                    tb.branch.clone(),
                    sha_a.clone(),
                    sha_b.clone(),
                ));
            }
        }
    }

    // Bounded concurrency semaphore (max 4 concurrent `git merge-tree` runs)
    let sem = Arc::new(tokio::sync::Semaphore::new(4));
    let mut handles = Vec::new();
    for (id_a, id_b, br_a, br_b, sha_a, sha_b) in pairs_to_compute {
        let sem_cl = sem.clone();
        let rp = repo_path.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem_cl.acquire().await.ok();
            let outcome = tokio::task::spawn_blocking(move || forecast_pair(&rp, &br_a, &br_b))
                .await
                .unwrap_or(MergeTreeOutcome::Error {
                    message: "merge-tree worker panicked".to_string(),
                });
            let now = now_rfc3339();
            match outcome {
                MergeTreeOutcome::Clean => ConflictCell {
                    task_a_id: id_a,
                    task_b_id: id_b,
                    sha_a,
                    sha_b,
                    status: ConflictStatus::Clean,
                    files: Vec::new(),
                    error_message: None,
                    computed_at: now,
                },
                MergeTreeOutcome::Conflicted { files } => ConflictCell {
                    task_a_id: id_a,
                    task_b_id: id_b,
                    sha_a,
                    sha_b,
                    status: ConflictStatus::Conflicted,
                    files,
                    error_message: None,
                    computed_at: now,
                },
                MergeTreeOutcome::Error { message } => ConflictCell {
                    task_a_id: id_a,
                    task_b_id: id_b,
                    sha_a,
                    sha_b,
                    status: ConflictStatus::Error,
                    files: Vec::new(),
                    error_message: Some(message),
                    computed_at: now,
                },
            }
        }));
    }

    for h in handles {
        if let Ok(cell) = h.await {
            let _ = state.store.save_cached_conflict(&repo.id, &cell);
            cells.push(cell);
        }
    }

    let suggestion = suggest_merge_order(&metrics, &cells);
    let report = ConflictMatrixReport {
        repo_id,
        cells,
        suggestion,
    };
    let _ = app.emit("conflicts:updated", report.clone());
    Ok(report)
}

#[tauri::command]
fn queue_status(state: State<'_, Arc<AppCore>>) -> CmdResult<QueueState> {
    Ok(state.queue.snapshot())
}

#[tauri::command]
async fn queue_start(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    repo_id: String,
    task_ids: Vec<String>,
) -> CmdResult<QueueState> {
    let repo = state.store.get_repo(&repo_id).map_err(map_err)?;
    let app_q = app.clone();
    let app_t = app.clone();

    state
        .queue
        .execute_queue(
            state.store.clone(),
            repo,
            task_ids,
            state.logs_dir.clone(),
            move |q_state| {
                let _ = app_q.emit("queue:progress", q_state);
            },
            move |task, reason| {
                let _ = app_t.emit("task:state", TaskStatePayload { task, reason });
            },
        )
        .await
        .map_err(map_err)
}

#[tauri::command]
fn queue_pause(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    paused: bool,
) -> CmdResult<QueueState> {
    let snap = state.queue.set_paused(paused);
    let _ = app.emit("queue:progress", snap.clone());
    Ok(snap)
}

#[tauri::command]
fn queue_stop(app: AppHandle, state: State<'_, Arc<AppCore>>) -> CmdResult<QueueState> {
    let snap = state.queue.request_stop_after_current();
    let _ = app.emit("queue:progress", snap.clone());
    Ok(snap)
}

#[tauri::command]
fn settings_get(state: State<'_, Arc<AppCore>>) -> CmdResult<AppSettings> {
    state.store.get_settings().map_err(map_err)
}

#[tauri::command]
fn settings_save(
    app: AppHandle,
    state: State<'_, Arc<AppCore>>,
    settings: AppSettings,
) -> CmdResult<AppSettings> {
    state.store.save_settings(&settings).map_err(map_err)?;
    if let Ok(repos) = state.store.list_repos() {
        for r in repos {
            state.schedule_queued_tasks(&app, &r.id);
        }
    }
    state.store.get_settings().map_err(map_err)
}

#[tauri::command]
fn events_list(state: State<'_, Arc<AppCore>>, limit: Option<usize>) -> CmdResult<Vec<TaskEvent>> {
    state
        .store
        .list_recent_events(limit.unwrap_or(100))
        .map_err(map_err)
}

#[tauri::command]
fn task_commit_all(
    state: State<'_, Arc<AppCore>>,
    task_id: String,
    message: String,
) -> CmdResult<String> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let wt = Path::new(&task.worktree_path);
    let add_out = run_git(wt, &["add", "."]).map_err(map_err)?;
    if !add_out.status.success() {
        return Err(AppError::with_details(
            "GIT_ADD_FAILED",
            "Couldn't stage files in the task worktree.",
            String::from_utf8_lossy(&add_out.stderr).to_string(),
        )
        .to_payload());
    }
    let commit_msg = if message.trim().is_empty() {
        task.title.clone()
    } else {
        message
    };
    let commit_out = run_git(wt, &["commit", "-m", &commit_msg]).map_err(map_err)?;
    if !commit_out.status.success() {
        let stderr = String::from_utf8_lossy(&commit_out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&commit_out.stdout).to_string();
        return Err(AppError::with_details(
            "GIT_COMMIT_FAILED",
            "Nothing to commit or git commit failed.",
            format!("{stdout} {stderr}"),
        )
        .to_payload());
    }
    let sha_out = run_git(wt, &["rev-parse", "HEAD"]).map_err(map_err)?;
    Ok(String::from_utf8_lossy(&sha_out.stdout).trim().to_string())
}

#[tauri::command]
fn backup_save_manual(state: State<'_, Arc<AppCore>>, task_id: String) -> CmdResult<String> {
    let task = state.store.get_task(&task_id).map_err(map_err)?;
    let repo = state.store.get_repo(&task.repo_id).map_err(map_err)?;
    let patch = save_backup_patch(
        Path::new(&task.worktree_path),
        Some(&repo.base_branch),
        &state.backups_dir.join(&repo.id),
        &task.branch,
    )
    .map_err(map_err)?;
    Ok(patch.to_string_lossy().to_string())
}

#[tauri::command]
fn diagnostics_export(
    state: State<'_, Arc<AppCore>>,
    destination_path: Option<String>,
) -> CmdResult<String> {
    let dest = destination_path
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let ts = now_compact_utc();
            state
                .data_dir
                .join(format!("beehaven-diagnostics-{ts}.json"))
        });
    let written =
        export_diagnostics_bundle(&state.store, &state.logs_dir, &dest).map_err(map_err)?;
    Ok(written.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("beehaven-data"));
            std::fs::create_dir_all(&data_dir)?;
            let logs_dir = data_dir.join("logs");
            let backups_dir = data_dir.join("backups");
            std::fs::create_dir_all(&logs_dir)?;
            std::fs::create_dir_all(&backups_dir)?;

            let log_guard = init_logging(&logs_dir);
            let db_path = data_dir.join("beehaven.sqlite3");
            let store = Arc::new(
                Store::open(&db_path).expect("Failed to initialize Beehaven SQLite database"),
            );

            // Crash recovery on startup (Section 4.3)
            if let Ok(recovered) = store.reconcile_on_startup() {
                if !recovered.is_empty() {
                    tracing::info!("Recovered {} tasks on startup", recovered.len());
                }
            }

            let hook_token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
                .replace('-', "");
            let (hook_tx, mut hook_rx) = mpsc::unbounded_channel::<HookMessage>();
            let hook_port = tauri::async_runtime::block_on(start_hook_listener(
                hook_token.clone(),
                hook_tx,
            ))
            .unwrap_or(0);

            let (pty_tx, mut pty_rx) = mpsc::unbounded_channel::<SessionCallbackEvent>();
            let sessions = SessionManager::new(logs_dir.clone());
            let queue = MergeQueueController::new();

            let core = Arc::new(AppCore {
                store: store.clone(),
                sessions,
                queue,
                data_dir,
                logs_dir,
                backups_dir,
                hook_port,
                hook_token,
                pty_event_tx: pty_tx,
                _log_guard: Mutex::new(log_guard),
            });

            // Background pump for PTY output, child exits, and silence detection
            let app_for_pty = app.handle().clone();
            let core_for_pty = core.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = pty_rx.recv().await {
                    match ev {
                        SessionCallbackEvent::Output { task_id, chunk } => {
                            let _ = app_for_pty.emit(
                                "session:output",
                                SessionOutputPayload { task_id, chunk },
                            );
                        }
                        SessionCallbackEvent::SilenceTimeout { task_id } => {
                            if let Ok(task) = core_for_pty.store.get_task(&task_id) {
                                if task.state == TaskState::Running {
                                    let adapter = get_adapter(&task.agent);
                                    if let Some(next) =
                                        adapter.interpret(&AgentSignal::SilenceTimeout)
                                    {
                                        if let Ok(updated) =
                                            core_for_pty.store.transition_task_state(
                                                &task_id,
                                                next,
                                                "No terminal output recently — agent may be waiting for input",
                                                None,
                                                None,
                                            )
                                        {
                                            let _ = app_for_pty.emit(
                                                "task:state",
                                                TaskStatePayload {
                                                    task: updated,
                                                    reason: "Agent may be waiting for input"
                                                        .to_string(),
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        SessionCallbackEvent::Exited {
                            task_id,
                            session_id,
                            exit_code,
                        } => {
                            let _ = core_for_pty.store.record_session_end(&session_id, exit_code);
                            if let Ok(task) = core_for_pty.store.get_task(&task_id) {
                                if matches!(task.state, TaskState::Running | TaskState::WaitingForYou)
                                {
                                    let adapter = get_adapter(&task.agent);
                                    let target = adapter
                                        .interpret(&AgentSignal::Exited(exit_code))
                                        .unwrap_or(TaskState::NeedsReview);
                                    let err_msg = if target == TaskState::Failed {
                                        Some(format!(
                                            "Agent process exited with non-zero code {:?}",
                                            exit_code
                                        ))
                                    } else {
                                        None
                                    };
                                    if let Ok(updated) = core_for_pty.store.transition_task_state(
                                        &task_id,
                                        target,
                                        "Agent process finished",
                                        err_msg,
                                        None,
                                    ) {
                                        let repo_id = updated.repo_id.clone();
                                        let _ = app_for_pty.emit(
                                            "task:state",
                                            TaskStatePayload {
                                                task: updated,
                                                reason: "Agent process finished".to_string(),
                                            },
                                        );
                                        core_for_pty.schedule_queued_tasks(&app_for_pty, &repo_id);
                                    }
                                }
                            }
                        }
                    }
                }
            });

            // Background pump for authenticated local hook socket events
            let app_for_hooks = app.handle().clone();
            let core_for_hooks = core.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(msg) = hook_rx.recv().await {
                    if let Ok(task) = core_for_hooks.store.get_task(&msg.task_id) {
                        let adapter = get_adapter(&task.agent);
                        if let Some(next_state) =
                            adapter.interpret(&AgentSignal::HookEvent(msg.event.clone()))
                        {
                            if task.state != next_state && task.state.can_transition_to(next_state) {
                                let reason = format!("Agent hook '{}' received", msg.event);
                                if let Ok(updated) = core_for_hooks.store.transition_task_state(
                                    &task.id,
                                    next_state,
                                    &reason,
                                    None,
                                    None,
                                ) {
                                    let repo_id = updated.repo_id.clone();
                                    let _ = app_for_hooks.emit(
                                        "task:state",
                                        TaskStatePayload {
                                            task: updated,
                                            reason,
                                        },
                                    );
                                    if next_state == TaskState::NeedsReview {
                                        core_for_hooks
                                            .schedule_queued_tasks(&app_for_hooks, &repo_id);
                                    }
                                }
                            }
                        }
                    }
                }
            });

            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            repo_check,
            repo_open,
            repo_list,
            repo_create_demo,
            worktree_list,
            agents_list,
            task_list,
            task_create,
            task_start,
            task_set_manual_state,
            task_approve,
            task_revise,
            task_discard,
            task_commit_all,
            backup_save_manual,
            session_replay,
            session_write,
            session_resize,
            session_kill,
            diff_summary_get,
            diff_file_get,
            tests_run,
            tests_list,
            conflicts_compute,
            queue_status,
            queue_start,
            queue_pause,
            queue_stop,
            settings_get,
            settings_save,
            events_list,
            diagnostics_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Beehaven Tauri application");
}
