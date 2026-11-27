use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use crate::error::{AppError, AppResult};
use crate::git::safety::{normalize_existing_path, run_git, validate_child_path};
use crate::git::worktree::has_uncommitted_changes;
use crate::models::{QueueItem, QueueItemStatus, QueueState, Repo, TaskState};
use crate::runner::run_test_command;
use crate::store::Store;

#[derive(Clone)]
pub struct MergeQueueController {
    state: Arc<Mutex<QueueState>>,
    notify: Arc<Notify>,
}

impl Default for MergeQueueController {
    fn default() -> Self {
        Self::new()
    }
}

impl MergeQueueController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(QueueState {
                running: false,
                paused: false,
                stop_after_current: false,
                items: Vec::new(),
                halted_reason: None,
                halted_task_id: None,
            })),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn snapshot(&self) -> QueueState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_paused(&self, paused: bool) -> QueueState {
        let mut guard = self.state.lock().unwrap();
        guard.paused = paused;
        let snap = guard.clone();
        drop(guard);
        self.notify.notify_waiters();
        snap
    }

    pub fn request_stop_after_current(&self) -> QueueState {
        let mut guard = self.state.lock().unwrap();
        guard.stop_after_current = true;
        guard.paused = false;
        let snap = guard.clone();
        drop(guard);
        self.notify.notify_waiters();
        snap
    }

    /// Executes the sequential test-gated merge queue (Section 4.4 & Scope S6):
    /// For each approved task in order:
    /// 1. Verify base branch and task worktree are clean.
    /// 2. Rebase the task branch onto the current tip of `base_branch` (aborting cleanly on conflict).
    /// 3. Check out the rebased commit in a fresh temporary integration worktree and run `test_command`.
    /// 4. Only if tests pass, fast-forward `base_branch` (`git merge --ff-only` or atomic `update-ref` CAS).
    /// 5. On any conflict or test failure, stop immediately, mark the task `Failed` with logs, and leave
    ///    `base_branch` untouched at the last green task.
    pub async fn execute_queue<F, G>(
        &self,
        store: Arc<Store>,
        repo: Repo,
        task_ids: Vec<String>,
        logs_dir: PathBuf,
        mut on_queue_progress: F,
        mut on_task_state: G,
    ) -> AppResult<QueueState>
    where
        F: FnMut(QueueState) + Send,
        G: FnMut(crate::models::Task, String) + Send,
    {
        {
            let mut guard = self.state.lock().unwrap();
            if guard.running {
                return Err(AppError::user(
                    "QUEUE_ALREADY_RUNNING",
                    "The merge queue is already running.",
                ));
            }
            let mut items = Vec::new();
            for id in &task_ids {
                let t = store.get_task(id)?;
                if t.state != TaskState::Approved {
                    return Err(AppError::user(
                        "TASK_NOT_APPROVED",
                        format!(
                            "Task '{}' must be Approved before entering the merge queue (currently '{}').",
                            t.title,
                            t.state.label()
                        ),
                    ));
                }
                items.push(QueueItem {
                    task_id: t.id,
                    title: t.title,
                    branch: t.branch,
                    status: QueueItemStatus::Waiting,
                    detail: "Waiting in queue".to_string(),
                });
            }
            *guard = QueueState {
                running: true,
                paused: false,
                stop_after_current: false,
                items,
                halted_reason: None,
                halted_task_id: None,
            };
        }
        on_queue_progress(self.snapshot());

        let repo_path = normalize_existing_path(Path::new(&repo.path))?;
        let wt_root = PathBuf::from(&repo.worktree_root);
        std::fs::create_dir_all(&wt_root)?;
        let canon_wt_root = normalize_existing_path(&wt_root)?;

        for idx in 0..task_ids.len() {
            // Honor pause / stop-after-current between tasks
            loop {
                let (paused, stop_now) = {
                    let g = self.state.lock().unwrap();
                    (g.paused, g.stop_after_current && idx > 0)
                };
                if stop_now {
                    let mut g = self.state.lock().unwrap();
                    for rem in g.items.iter_mut().skip(idx) {
                        if rem.status == QueueItemStatus::Waiting {
                            rem.status = QueueItemStatus::Stopped;
                            rem.detail = "Stopped by user after previous task".to_string();
                        }
                    }
                    g.running = false;
                    g.halted_reason = Some("Stopped after current task as requested.".to_string());
                    let snap = g.clone();
                    drop(g);
                    on_queue_progress(snap.clone());
                    return Ok(snap);
                }
                if !paused {
                    break;
                }
                self.notify.notified().await;
            }

            let task_id = &task_ids[idx];
            let task = store.get_task(task_id)?;

            // Transition Approved -> Merging
            let merging_task = store.transition_task_state(
                task_id,
                TaskState::Merging,
                &format!(
                    "Merge queue step {} of {}: rebasing",
                    idx + 1,
                    task_ids.len()
                ),
                None,
                None,
            )?;
            on_task_state(merging_task, "Entered merge queue".to_string());
            self.update_item(
                idx,
                QueueItemStatus::Rebasing,
                format!(
                    "Rebasing task {} of {} onto {}",
                    idx + 1,
                    task_ids.len(),
                    repo.base_branch
                ),
                &mut on_queue_progress,
            );

            // Step 1: Ensure main repo working directory is safe if base_branch is checked out there
            if let Err(err) = ensure_base_ready_for_ff(&repo_path, &repo.base_branch) {
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    err.to_payload().message,
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            // Ensure task worktree has no uncommitted changes before rebase
            let task_wt = normalize_existing_path(Path::new(&task.worktree_path))?;
            if has_uncommitted_changes(&task_wt)? {
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    format!(
                        "Task '{}' has uncommitted files in its worktree. Commit or discard them before merging.",
                        task.title
                    ),
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            // Step 2: Rebase task branch onto current base_branch
            let rebase_out = run_git(&task_wt, &["rebase", &repo.base_branch])?;
            if !rebase_out.status.success() {
                let conflict_stderr = String::from_utf8_lossy(&rebase_out.stderr)
                    .trim()
                    .to_string();
                let conflict_stdout = String::from_utf8_lossy(&rebase_out.stdout)
                    .trim()
                    .to_string();
                let _ = run_git(&task_wt, &["rebase", "--abort"]);
                let reason = format!(
                    "Rebase onto '{}' hit a conflict. Aborted cleanly so no work was lost. ({}{})",
                    repo.base_branch,
                    conflict_stderr,
                    if conflict_stdout.is_empty() {
                        String::new()
                    } else {
                        format!(" {conflict_stdout}")
                    }
                );
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    reason,
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            // Step 3: Run configured test command inside an isolated temporary worktree!
            self.update_item(
                idx,
                QueueItemStatus::Testing,
                format!(
                    "Running '{}' in isolated verification worktree",
                    repo.test_command
                ),
                &mut on_queue_progress,
            );

            let temp_folder_name = format!(
                ".merge-queue-{}",
                task_id
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(10)
                    .collect::<String>()
            );
            let temp_wt = validate_child_path(&canon_wt_root, Path::new(&temp_folder_name))?;
            if temp_wt.exists() {
                let s = temp_wt.to_string_lossy().to_string();
                let _ = run_git(&repo_path, &["worktree", "remove", "--force", &s]);
            }

            let temp_wt_str = temp_wt.to_string_lossy().to_string();
            let add_temp = run_git(
                &repo_path,
                &["worktree", "add", "--detach", &temp_wt_str, &task.branch],
            )?;
            if !add_temp.status.success() {
                let err_msg = format!(
                    "Couldn't create temporary verification worktree: {}",
                    String::from_utf8_lossy(&add_temp.stderr).trim()
                );
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    err_msg,
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            let test_run_res = run_test_command(
                task_id,
                &temp_wt,
                &repo.test_command,
                &logs_dir,
                Duration::from_secs(300),
                |_chunk| {},
            )
            .await;

            // Always clean up temporary verification worktree immediately!
            let _ = run_git(&repo_path, &["worktree", "remove", "--force", &temp_wt_str]);

            let test_run = match test_run_res {
                Ok(r) => {
                    store.insert_test_run(&r)?;
                    r
                }
                Err(e) => {
                    return self.halt_with_failure(
                        &store,
                        idx,
                        task_id,
                        e.to_payload().message,
                        &mut on_queue_progress,
                        &mut on_task_state,
                    );
                }
            };

            if !test_run.passed {
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    format!(
                        "Verification tests failed after rebasing '{}' onto '{}'. Logs saved at {}.",
                        task.title, repo.base_branch, test_run.log_path
                    ),
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            // Step 4: Fast-forward base_branch (`git merge --ff-only` or atomic CAS ref update)
            self.update_item(
                idx,
                QueueItemStatus::Merging,
                format!(
                    "Fast-forwarding '{}' to '{}'",
                    repo.base_branch, task.branch
                ),
                &mut on_queue_progress,
            );

            if let Err(ff_err) = fast_forward_base(&repo_path, &repo.base_branch, &task.branch) {
                return self.halt_with_failure(
                    &store,
                    idx,
                    task_id,
                    ff_err.to_payload().message,
                    &mut on_queue_progress,
                    &mut on_task_state,
                );
            }

            let merged_task = store.transition_task_state(
                task_id,
                TaskState::Merged,
                "Merged cleanly via test-gated merge queue.",
                None,
                None,
            )?;
            on_task_state(merged_task, "Merged into base branch".to_string());
            self.update_item(
                idx,
                QueueItemStatus::Merged,
                "Merged, tests passed".to_string(),
                &mut on_queue_progress,
            );
        }

        let snap = {
            let mut g = self.state.lock().unwrap();
            g.running = false;
            g.paused = false;
            g.stop_after_current = false;
            g.clone()
        };
        on_queue_progress(snap.clone());
        Ok(snap)
    }

    fn update_item<F>(
        &self,
        idx: usize,
        status: QueueItemStatus,
        detail: String,
        on_progress: &mut F,
    ) where
        F: FnMut(QueueState),
    {
        let snap = {
            let mut g = self.state.lock().unwrap();
            if let Some(item) = g.items.get_mut(idx) {
                item.status = status;
                item.detail = detail;
            }
            g.clone()
        };
        on_progress(snap);
    }

    fn halt_with_failure<F, G>(
        &self,
        store: &Store,
        idx: usize,
        task_id: &str,
        reason: String,
        on_progress: &mut F,
        on_task_state: &mut G,
    ) -> AppResult<QueueState>
    where
        F: FnMut(QueueState),
        G: FnMut(crate::models::Task, String),
    {
        let failed_task = store.transition_task_state(
            task_id,
            TaskState::Failed,
            &reason,
            Some(reason.clone()),
            None,
        )?;
        on_task_state(failed_task, reason.clone());

        let snap = {
            let mut g = self.state.lock().unwrap();
            if let Some(item) = g.items.get_mut(idx) {
                item.status = QueueItemStatus::Failed;
                item.detail = reason.clone();
            }
            for rem in g.items.iter_mut().skip(idx + 1) {
                if rem.status == QueueItemStatus::Waiting {
                    rem.status = QueueItemStatus::Stopped;
                    rem.detail = "Queue halted due to failure in earlier task".to_string();
                }
            }
            g.running = false;
            g.paused = false;
            g.halted_reason = Some(reason);
            g.halted_task_id = Some(task_id.to_string());
            g.clone()
        };
        on_progress(snap.clone());
        Ok(snap)
    }
}

fn ensure_base_ready_for_ff(repo_path: &Path, base_branch: &str) -> AppResult<()> {
    let head_ref = run_git(repo_path, &["symbolic-ref", "--short", "HEAD"])?;
    let current = String::from_utf8_lossy(&head_ref.stdout).trim().to_string();
    if current == base_branch {
        // Check tracked modifications in main repo (untracked files that don't collide won't block,
        // so check `git status --porcelain --untracked-files=no`)
        let st = run_git(
            repo_path,
            &["status", "--porcelain", "--untracked-files=no"],
        )?;
        if !String::from_utf8_lossy(&st.stdout).trim().is_empty() {
            return Err(AppError::user(
                "MAIN_REPO_DIRTY",
                format!(
                    "Your main repository folder has uncommitted tracked edits on '{base_branch}'. Commit or stash them before merging."
                ),
            ));
        }
    }
    Ok(())
}

fn fast_forward_base(repo_path: &Path, base_branch: &str, task_branch: &str) -> AppResult<()> {
    let head_ref = run_git(repo_path, &["symbolic-ref", "--short", "HEAD"])?;
    let current = String::from_utf8_lossy(&head_ref.stdout).trim().to_string();

    if current == base_branch {
        let ff = run_git(repo_path, &["merge", "--ff-only", task_branch])?;
        if !ff.status.success() {
            return Err(AppError::with_details(
                "FF_MERGE_FAILED",
                format!(
                    "Fast-forward merge of '{task_branch}' into '{base_branch}' failed. Make sure no untracked files in your main folder overwrite incoming files."
                ),
                String::from_utf8_lossy(&ff.stderr).to_string(),
            ));
        }
        return Ok(());
    }

    // Main repo has a different branch checked out: enforce ancestor check + atomic CAS update-ref
    let anc = run_git(
        repo_path,
        &["merge-base", "--is-ancestor", base_branch, task_branch],
    )?;
    if !anc.status.success() {
        return Err(AppError::user(
            "NOT_FAST_FORWARD",
            format!("'{task_branch}' is not a fast-forward descendant of '{base_branch}'."),
        ));
    }

    let old_out = run_git(repo_path, &["rev-parse", base_branch])?;
    let new_out = run_git(repo_path, &["rev-parse", task_branch])?;
    let old_sha = String::from_utf8_lossy(&old_out.stdout).trim().to_string();
    let new_sha = String::from_utf8_lossy(&new_out.stdout).trim().to_string();
    let full_ref = format!("refs/heads/{base_branch}");
    let upd = run_git(repo_path, &["update-ref", &full_ref, &new_sha, &old_sha])?;
    if !upd.status.success() {
        return Err(AppError::with_details(
            "UPDATE_REF_FAILED",
            format!("Couldn't fast-forward '{base_branch}' atomically."),
            String::from_utf8_lossy(&upd.stderr).to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::worktree::{create_worktree, default_worktree_root};
    use crate::models::Task;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} in {:?} failed: {}",
            args,
            dir,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn make_approved_task(
        store: &Store,
        repo: &Repo,
        id: &str,
        title: &str,
        file_name: &str,
        content: &str,
    ) -> Task {
        let repo_path = Path::new(&repo.path);
        let wt_root = Path::new(&repo.worktree_root);
        let (branch, wt_path) =
            create_worktree(repo_path, wt_root, &repo.base_branch, id, title).unwrap();
        fs::write(wt_path.join(file_name), content).unwrap();
        git(&wt_path, &["add", "."]);
        git(&wt_path, &["commit", "-m", title]);

        let t = Task {
            id: id.into(),
            repo_id: repo.id.clone(),
            title: title.into(),
            prompt: title.into(),
            branch,
            worktree_path: wt_path.to_string_lossy().into(),
            agent: "generic".into(),
            state: TaskState::Queued,
            backup_patch_path: None,
            last_error: None,
            created_at: "2026-11-27T00:00:00Z".into(),
            updated_at: "2026-11-27T00:00:00Z".into(),
        };
        store.insert_task(&t).unwrap();
        store
            .transition_task_state(id, TaskState::Running, "start", None, None)
            .unwrap();
        store
            .transition_task_state(id, TaskState::NeedsReview, "done", None, None)
            .unwrap();
        store
            .transition_task_state(id, TaskState::Approved, "approve", None, None)
            .unwrap()
    }

    #[tokio::test]
    async fn merge_queue_merges_three_linearly_and_halts_on_test_failure_or_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_dir = tmp.path().join("sample-repo");
        fs::create_dir_all(&repo_dir).unwrap();
        git(&repo_dir, &["init", "-b", "main"]);
        git(&repo_dir, &["config", "user.name", "Tester"]);
        git(&repo_dir, &["config", "user.email", "tester@example.com"]);
        fs::write(
            repo_dir.join("check.mjs"),
            "import fs from 'node:fs'; if (fs.existsSync('FAIL_TEST')) process.exit(1); process.exit(0);\n",
        )
        .unwrap();
        fs::write(repo_dir.join("shared.txt"), "base line\n").unwrap();
        git(&repo_dir, &["add", "."]);
        git(&repo_dir, &["commit", "-m", "initial"]);

        let wt_root = default_worktree_root(&repo_dir).unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let repo = store
            .upsert_repo(&Repo {
                id: "r1".into(),
                name: "sample-repo".into(),
                path: normalize_existing_path(&repo_dir)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
                base_branch: "main".into(),
                test_command: "node check.mjs".into(),
                worktree_root: wt_root.to_string_lossy().into(),
                created_at: "2026-11-27T00:00:00Z".into(),
            })
            .unwrap();

        // Part 1: Merge 3 clean tasks in order -> linear history!
        make_approved_task(&store, &repo, "t1", "Docs", "docs.md", "# Docs\n");
        make_approved_task(
            &store,
            &repo,
            "t2",
            "Parser",
            "parser.js",
            "export const x = 1;\n",
        );
        make_approved_task(
            &store,
            &repo,
            "t3",
            "Refactor",
            "refactor.js",
            "export const y = 2;\n",
        );

        let queue = MergeQueueController::new();
        let final_state = queue
            .execute_queue(
                store.clone(),
                repo.clone(),
                vec!["t1".into(), "t2".into(), "t3".into()],
                tmp.path().join("logs"),
                |_| {},
                |_, _| {},
            )
            .await
            .unwrap();

        assert!(final_state.halted_reason.is_none());
        assert_eq!(store.get_task("t1").unwrap().state, TaskState::Merged);
        assert_eq!(store.get_task("t2").unwrap().state, TaskState::Merged);
        assert_eq!(store.get_task("t3").unwrap().state, TaskState::Merged);

        // Verify linear history (zero merge commits!)
        let merge_commits = run_git(&repo_dir, &["log", "--merges", "--oneline", "main"]).unwrap();
        assert!(
            String::from_utf8_lossy(&merge_commits.stdout)
                .trim()
                .is_empty(),
            "History on main must be strictly linear"
        );

        // Part 2: Failing test in task 5 stops queue leaving main at task 4!
        make_approved_task(&store, &repo, "t4", "Good feature", "good.txt", "ok\n");
        make_approved_task(&store, &repo, "t5", "Broken test", "FAIL_TEST", "boom\n");
        make_approved_task(
            &store,
            &repo,
            "t6",
            "After broken",
            "after.txt",
            "skipped\n",
        );

        let state2 = queue
            .execute_queue(
                store.clone(),
                repo.clone(),
                vec!["t4".into(), "t5".into(), "t6".into()],
                tmp.path().join("logs"),
                |_| {},
                |_, _| {},
            )
            .await
            .unwrap();

        assert!(state2.halted_reason.is_some());
        assert_eq!(state2.halted_task_id.as_deref(), Some("t5"));
        assert_eq!(store.get_task("t4").unwrap().state, TaskState::Merged);
        assert_eq!(store.get_task("t5").unwrap().state, TaskState::Failed);
        assert_eq!(store.get_task("t6").unwrap().state, TaskState::Approved);

        let head_msg_out = run_git(&repo_dir, &["log", "-1", "--pretty=%s", "main"]).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head_msg_out.stdout).trim(),
            "Good feature",
            "Base branch must stop cleanly at task 4"
        );

        // Part 3: Conflict stops queue cleanly and aborts rebase!
        make_approved_task(
            &store,
            &repo,
            "t7",
            "Edit shared 1",
            "shared.txt",
            "version 1\n",
        );
        make_approved_task(
            &store,
            &repo,
            "t8",
            "Edit shared 2",
            "shared.txt",
            "version 2\n",
        );

        let state3 = queue
            .execute_queue(
                store.clone(),
                repo.clone(),
                vec!["t7".into(), "t8".into()],
                tmp.path().join("logs"),
                |_| {},
                |_, _| {},
            )
            .await
            .unwrap();

        assert_eq!(state3.halted_task_id.as_deref(), Some("t8"));
        assert_eq!(store.get_task("t7").unwrap().state, TaskState::Merged);
        assert_eq!(store.get_task("t8").unwrap().state, TaskState::Failed);
        // Verify t8 worktree is clean (rebase --abort succeeded)
        let t8_wt = PathBuf::from(store.get_task("t8").unwrap().worktree_path);
        assert!(!has_uncommitted_changes(&t8_wt).unwrap());
    }
}
