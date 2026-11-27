use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::models::{
    AppSettings, ConflictCell, ConflictStatus, Repo, SessionRecord, Task, TaskEvent, TaskState,
    TestRun,
};
use crate::time_util::now_rfc3339;

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct CachedConflict {
    pub status: ConflictStatus,
    pub files: Vec<String>,
    pub error_message: Option<String>,
    pub computed_at: String,
}

impl Store {
    pub fn open(db_path: &Path) -> AppResult<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repos (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                base_branch TEXT NOT NULL,
                test_command TEXT NOT NULL,
                worktree_root TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                prompt TEXT NOT NULL,
                branch TEXT NOT NULL,
                worktree_path TEXT NOT NULL,
                agent TEXT NOT NULL,
                state TEXT NOT NULL,
                backup_patch_path TEXT,
                last_error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                pid INTEGER,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                exit_code INTEGER
            );

            CREATE TABLE IF NOT EXISTS test_runs (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                commit_sha TEXT NOT NULL,
                passed INTEGER NOT NULL,
                log_path TEXT NOT NULL,
                output_preview TEXT NOT NULL,
                started_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conflicts (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
                sha_a TEXT NOT NULL,
                sha_b TEXT NOT NULL,
                status TEXT NOT NULL,
                files_json TEXT NOT NULL,
                error_message TEXT,
                computed_at TEXT NOT NULL,
                UNIQUE(repo_id, sha_a, sha_b)
            );

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                at TEXT NOT NULL,
                kind TEXT NOT NULL,
                detail_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn upsert_repo(&self, repo: &Repo) -> AppResult<Repo> {
        let conn = self.conn.lock().unwrap();
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM repos WHERE path = ?1",
                params![repo.path],
                |row| row.get(0),
            )
            .optional()?;

        let final_id = existing_id.unwrap_or_else(|| repo.id.clone());
        conn.execute(
            "INSERT INTO repos (id, name, path, base_branch, test_command, worktree_root, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                base_branch = excluded.base_branch,
                test_command = excluded.test_command,
                worktree_root = excluded.worktree_root",
            params![
                final_id,
                repo.name,
                repo.path,
                repo.base_branch,
                repo.test_command,
                repo.worktree_root,
                repo.created_at
            ],
        )?;
        drop(conn);
        self.get_repo(&final_id)
    }

    pub fn get_repo(&self, repo_id: &str) -> AppResult<Repo> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, path, base_branch, test_command, worktree_root, created_at
             FROM repos WHERE id = ?1",
            params![repo_id],
            |row| {
                Ok(Repo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    base_branch: row.get(3)?,
                    test_command: row.get(4)?,
                    worktree_root: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
        .map_err(|e| {
            AppError::with_details(
                "REPO_NOT_FOUND",
                format!("Repository '{repo_id}' was not found in the database."),
                e.to_string(),
            )
        })
    }

    pub fn list_repos(&self) -> AppResult<Vec<Repo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, base_branch, test_command, worktree_root, created_at
             FROM repos ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Repo {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                base_branch: row.get(3)?,
                test_command: row.get(4)?,
                worktree_root: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_task(&self, task: &Task) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tasks (
                id, repo_id, title, prompt, branch, worktree_path, agent, state,
                backup_patch_path, last_error, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                task.id,
                task.repo_id,
                task.title,
                task.prompt,
                task.branch,
                task.worktree_path,
                task.agent,
                task.state.as_str(),
                task.backup_patch_path,
                task.last_error,
                task.created_at,
                task.updated_at,
            ],
        )?;
        conn.execute(
            "INSERT INTO events (task_id, at, kind, detail_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                task.id,
                task.created_at,
                "task:created",
                serde_json::json!({ "state": task.state.as_str(), "agent": task.agent })
                    .to_string()
            ],
        )?;
        Ok(())
    }

    fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
        let raw_state: String = row.get(7)?;
        let state = TaskState::parse(&raw_state).unwrap_or(TaskState::Failed);
        Ok(Task {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            title: row.get(2)?,
            prompt: row.get(3)?,
            branch: row.get(4)?,
            worktree_path: row.get(5)?,
            agent: row.get(6)?,
            state,
            backup_patch_path: row.get(8)?,
            last_error: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    }

    pub fn get_task(&self, task_id: &str) -> AppResult<Task> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, repo_id, title, prompt, branch, worktree_path, agent, state,
                    backup_patch_path, last_error, created_at, updated_at
             FROM tasks WHERE id = ?1",
            params![task_id],
            Self::row_to_task,
        )
        .map_err(|e| {
            AppError::with_details(
                "TASK_NOT_FOUND",
                format!("Task '{task_id}' was not found."),
                e.to_string(),
            )
        })
    }

    pub fn list_tasks(&self, repo_id: &str) -> AppResult<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repo_id, title, prompt, branch, worktree_path, agent, state,
                    backup_patch_path, last_error, created_at, updated_at
             FROM tasks WHERE repo_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![repo_id], Self::row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_all_tasks(&self) -> AppResult<Vec<Task>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, repo_id, title, prompt, branch, worktree_path, agent, state,
                    backup_patch_path, last_error, created_at, updated_at
             FROM tasks ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_task)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Single gatekeeper for ALL task state transitions (Section 4.3).
    /// Enforces `TaskState::can_transition_to`, updates `tasks`, and writes an immutable `events` entry.
    pub fn transition_task_state(
        &self,
        task_id: &str,
        next: TaskState,
        reason: &str,
        last_error: Option<String>,
        backup_patch_path: Option<String>,
    ) -> AppResult<Task> {
        let current = self.get_task(task_id)?;
        if !current.state.can_transition_to(next) {
            return Err(AppError::user(
                "INVALID_STATE_TRANSITION",
                format!(
                    "Cannot move task '{}' from '{}' to '{}'.",
                    current.title,
                    current.state.label(),
                    next.label()
                ),
            ));
        }

        let now = now_rfc3339();
        let merged_patch = backup_patch_path.or(current.backup_patch_path.clone());
        let merged_err = if next == TaskState::Failed {
            last_error.or(current.last_error.clone())
        } else if next == TaskState::Running
            || next == TaskState::Approved
            || next == TaskState::Merged
        {
            None
        } else {
            last_error.or(current.last_error.clone())
        };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tasks
             SET state = ?1, last_error = ?2, backup_patch_path = ?3, updated_at = ?4
             WHERE id = ?5",
            params![next.as_str(), merged_err, merged_patch, now, task_id],
        )?;
        conn.execute(
            "INSERT INTO events (task_id, at, kind, detail_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                task_id,
                now,
                "task:state",
                serde_json::json!({
                    "from": current.state.as_str(),
                    "to": next.as_str(),
                    "reason": reason,
                })
                .to_string()
            ],
        )?;
        drop(conn);
        self.get_task(task_id)
    }

    pub fn record_session_start(&self, session: &SessionRecord) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, task_id, pid, started_at, ended_at, exit_code)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.id,
                session.task_id,
                session.pid,
                session.started_at,
                session.ended_at,
                session.exit_code
            ],
        )?;
        Ok(())
    }

    pub fn record_session_end(&self, session_id: &str, exit_code: Option<i32>) -> AppResult<()> {
        let now = now_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET ended_at = ?1, exit_code = ?2 WHERE id = ?3",
            params![now, exit_code, session_id],
        )?;
        Ok(())
    }

    pub fn insert_test_run(&self, run: &TestRun) -> AppResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO test_runs (
                id, task_id, commit_sha, passed, log_path, output_preview, started_at, duration_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id,
                run.task_id,
                run.commit_sha,
                if run.passed { 1 } else { 0 },
                run.log_path,
                run.output_preview,
                run.started_at,
                run.duration_ms
            ],
        )?;
        Ok(())
    }

    pub fn list_test_runs(&self, task_id: &str) -> AppResult<Vec<TestRun>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, commit_sha, passed, log_path, output_preview, started_at, duration_ms
             FROM test_runs WHERE task_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            let passed_int: i64 = row.get(3)?;
            Ok(TestRun {
                id: row.get(0)?,
                task_id: row.get(1)?,
                commit_sha: row.get(2)?,
                passed: passed_int != 0,
                log_path: row.get(4)?,
                output_preview: row.get(5)?,
                started_at: row.get(6)?,
                duration_ms: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn canonical_sha_pair<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }

    pub fn get_cached_conflict(
        &self,
        repo_id: &str,
        sha_a: &str,
        sha_b: &str,
    ) -> AppResult<Option<CachedConflict>> {
        let (s1, s2) = Self::canonical_sha_pair(sha_a, sha_b);
        let conn = self.conn.lock().unwrap();
        let res = conn
            .query_row(
                "SELECT status, files_json, error_message, computed_at
                 FROM conflicts WHERE repo_id = ?1 AND sha_a = ?2 AND sha_b = ?3",
                params![repo_id, s1, s2],
                |row| {
                    let status_raw: String = row.get(0)?;
                    let files_json: String = row.get(1)?;
                    let error_message: Option<String> = row.get(2)?;
                    let computed_at: String = row.get(3)?;
                    let status = match status_raw.as_str() {
                        "Clean" => ConflictStatus::Clean,
                        "Conflicted" => ConflictStatus::Conflicted,
                        _ => ConflictStatus::Error,
                    };
                    let files: Vec<String> = serde_json::from_str(&files_json).unwrap_or_default();
                    Ok(CachedConflict {
                        status,
                        files,
                        error_message,
                        computed_at,
                    })
                },
            )
            .optional()?;
        Ok(res)
    }

    pub fn save_cached_conflict(&self, repo_id: &str, cell: &ConflictCell) -> AppResult<()> {
        let (s1, s2) = Self::canonical_sha_pair(&cell.sha_a, &cell.sha_b);
        let status_str = match cell.status {
            ConflictStatus::Clean => "Clean",
            ConflictStatus::Conflicted => "Conflicted",
            ConflictStatus::Error => "Error",
        };
        let files_json = serde_json::to_string(&cell.files)?;
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conflicts (id, repo_id, sha_a, sha_b, status, files_json, error_message, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(repo_id, sha_a, sha_b) DO UPDATE SET
                status = excluded.status,
                files_json = excluded.files_json,
                error_message = excluded.error_message,
                computed_at = excluded.computed_at",
            params![
                id,
                repo_id,
                s1,
                s2,
                status_str,
                files_json,
                cell.error_message,
                cell.computed_at
            ],
        )?;
        Ok(())
    }

    pub fn get_settings(&self) -> AppResult<AppSettings> {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'app_settings'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(json) = raw {
            Ok(serde_json::from_str(&json).unwrap_or_default())
        } else {
            Ok(AppSettings::default())
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> AppResult<()> {
        let json = serde_json::to_string(settings)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value_json) VALUES ('app_settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![json],
        )?;
        Ok(())
    }

    pub fn list_recent_events(&self, limit: usize) -> AppResult<Vec<TaskEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, at, kind, detail_json
             FROM events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(TaskEvent {
                id: row.get(0)?,
                task_id: row.get(1)?,
                at: row.get(2)?,
                kind: row.get(3)?,
                detail_json: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Startup crash recovery (Section 4.3 & Scope S6):
    /// Reconciles tasks stuck in `Running`, `WaitingForYou`, or `Merging` when Beehaven restarts,
    /// aborts any half-finished rebase, and cleans up orphaned `.merge-queue-*` worktrees.
    pub fn reconcile_on_startup(&self) -> AppResult<Vec<Task>> {
        let mut recovered = Vec::new();
        for repo in self.list_repos()? {
            let wt_root = Path::new(&repo.worktree_root);
            let repo_path = Path::new(&repo.path);
            if wt_root.exists() {
                if let Ok(entries) = std::fs::read_dir(wt_root) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(".merge-queue-") {
                            let p = entry.path().to_string_lossy().to_string();
                            let _ = crate::git::safety::run_git(
                                repo_path,
                                &["worktree", "remove", "--force", &p],
                            );
                        }
                    }
                }
            }
        }

        for task in self.list_all_tasks()? {
            let wt = Path::new(&task.worktree_path);
            match task.state {
                TaskState::Running | TaskState::WaitingForYou => {
                    if wt.exists() {
                        let updated = self.transition_task_state(
                            &task.id,
                            TaskState::NeedsReview,
                            "Recovered after app restart; previous PTY session ended.",
                            None,
                            None,
                        )?;
                        recovered.push(updated);
                    } else {
                        let updated = self.transition_task_state(
                            &task.id,
                            TaskState::Failed,
                            "Worktree missing on startup crash check.",
                            Some("Worktree directory was missing on startup.".to_string()),
                            None,
                        )?;
                        recovered.push(updated);
                    }
                }
                TaskState::Merging => {
                    if wt.exists() && (wt.join(".git").exists() || wt.join("rebase-merge").exists())
                    {
                        let _ = crate::git::safety::run_git(wt, &["rebase", "--abort"]);
                    }
                    let next = if wt.exists() {
                        TaskState::Approved
                    } else {
                        TaskState::Failed
                    };
                    let updated = self.transition_task_state(
                        &task.id,
                        next,
                        "Merge queue interrupted by app restart; safely rolled back.",
                        Some("Merge queue was interrupted by a restart.".to_string()),
                        None,
                    )?;
                    recovered.push(updated);
                }
                _ => {}
            }
        }
        Ok(recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_persists_tasks_enforces_transitions_and_recovers_crashes() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_dir = tmp.path().join("wt-1");
        std::fs::create_dir_all(&wt_dir).unwrap();

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .upsert_repo(&Repo {
                id: "r1".into(),
                name: "demo".into(),
                path: tmp.path().to_string_lossy().into(),
                base_branch: "main".into(),
                test_command: "cargo test".into(),
                worktree_root: tmp.path().join("wts").to_string_lossy().into(),
                created_at: "2026-11-27T00:00:00Z".into(),
            })
            .unwrap();

        let t = Task {
            id: "t1".into(),
            repo_id: repo.id.clone(),
            title: "Login".into(),
            prompt: "Add validation".into(),
            branch: "beehaven/t1-login".into(),
            worktree_path: wt_dir.to_string_lossy().into(),
            agent: "generic".into(),
            state: TaskState::Queued,
            backup_patch_path: None,
            last_error: None,
            created_at: "2026-11-27T00:00:00Z".into(),
            updated_at: "2026-11-27T00:00:00Z".into(),
        };
        store.insert_task(&t).unwrap();

        // Illegal jump Queued -> Merged must fail
        assert!(store
            .transition_task_state("t1", TaskState::Merged, "bad", None, None)
            .is_err());

        // Valid move Queued -> Running
        let running = store
            .transition_task_state("t1", TaskState::Running, "slot free", None, None)
            .unwrap();
        assert_eq!(running.state, TaskState::Running);

        // Simulate crash recovery
        let recovered = store.reconcile_on_startup().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].state, TaskState::NeedsReview);
    }
}
