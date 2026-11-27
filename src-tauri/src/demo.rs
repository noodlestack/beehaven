use std::path::Path;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::git::safety::{normalize_existing_path, run_git};
use crate::git::worktree::{create_worktree, default_worktree_root};
use crate::models::{Repo, Task, TaskState};
use crate::runner::run_test_command;
use crate::store::Store;
use crate::time_util::{now_compact_utc, now_rfc3339};

fn node_available() -> bool {
    let mut cmd = std::process::Command::new("node");
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Creates a real, self-contained Git sandbox repository on disk with 4 isolated worktrees
/// (including real diffs, passing tests, and a deliberate pairwise merge conflict) so any
/// user who installs Beehaven can explore every feature safely in one click!
pub async fn create_demo_sandbox(
    data_dir: &Path,
    logs_dir: &Path,
    store: &Store,
) -> AppResult<Repo> {
    let stamp = now_compact_utc();
    let repo_dir = data_dir
        .join("sandboxes")
        .join(format!("honeycomb-demo-{stamp}"));
    std::fs::create_dir_all(&repo_dir)?;
    std::fs::create_dir_all(repo_dir.join("src"))?;

    let init_out = run_git(&repo_dir, &["init", "-b", "main"])?;
    if !init_out.status.success() {
        return Err(AppError::with_details(
            "DEMO_INIT_FAILED",
            "Couldn't initialize the practice Git repository.",
            String::from_utf8_lossy(&init_out.stderr).to_string(),
        ));
    }
    let _ = run_git(&repo_dir, &["config", "user.name", "Beehaven Sandbox"]);
    let _ = run_git(
        &repo_dir,
        &["config", "user.email", "sandbox@beehaven.local"],
    );

    std::fs::write(
        repo_dir.join("README.md"),
        "# Honeycomb Practice Repo\n\nWelcome to your safe Beehaven practice sandbox! Every branch and folder here is a real Git worktree.\n",
    )?;
    std::fs::write(
        repo_dir.join("src").join("nectar.mjs"),
        "export function nectarScore(flowers) {\n  return flowers * 2;\n}\n",
    )?;
    std::fs::write(
        repo_dir.join("src").join("banner.mjs"),
        "export function bannerText() {\n  return 'Welcome to Honeycomb v1';\n}\n",
    )?;
    std::fs::write(
        repo_dir.join("test-suite.mjs"),
        "import { nectarScore } from './src/nectar.mjs';\nif (nectarScore(5) < 10) {\n  console.error('nectarScore failed');\n  process.exit(1);\n}\nconsole.log('✔ All Honeycomb checks passed (3 assertions)');\n",
    )?;

    let _ = run_git(&repo_dir, &["add", "."])?;
    let _ = run_git(&repo_dir, &["commit", "-m", "Initial Honeycomb baseline"])?;

    let canon_repo = normalize_existing_path(&repo_dir)?;
    let wt_root = default_worktree_root(&canon_repo)?;
    let test_command = if node_available() {
        "node test-suite.mjs".to_string()
    } else {
        "git status --short".to_string()
    };

    let repo = store.upsert_repo(&Repo {
        id: uuid::Uuid::new_v4().to_string(),
        name: "honeycomb-practice".to_string(),
        path: canon_repo.to_string_lossy().to_string(),
        base_branch: "main".to_string(),
        test_command: test_command.clone(),
        worktree_root: wt_root.to_string_lossy().to_string(),
        created_at: now_rfc3339(),
    })?;

    // Helper to create a real worktree task with a real git commit
    let make_task = |title: &str,
                     prompt: &str,
                     rel_file: &str,
                     new_content: &str,
                     final_state: TaskState|
     -> AppResult<Task> {
        let tid = uuid::Uuid::new_v4().to_string();
        let (branch, wt_path) = create_worktree(&canon_repo, &wt_root, "main", &tid, title)?;
        let target_file = wt_path.join(rel_file);
        if let Some(p) = target_file.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&target_file, new_content)?;
        let _ = run_git(&wt_path, &["add", "."])?;
        let _ = run_git(&wt_path, &["commit", "-m", title])?;

        let now = now_rfc3339();
        let task = Task {
            id: tid.clone(),
            repo_id: repo.id.clone(),
            title: title.to_string(),
            prompt: prompt.to_string(),
            branch,
            worktree_path: wt_path.to_string_lossy().to_string(),
            agent: "generic".to_string(),
            state: TaskState::Queued,
            backup_patch_path: None,
            last_error: None,
            created_at: now.clone(),
            updated_at: now,
        };
        store.insert_task(&task)?;
        store.transition_task_state(&tid, TaskState::Running, "Demo setup", None, None)?;
        if final_state == TaskState::WaitingForYou {
            return store.transition_task_state(
                &tid,
                TaskState::WaitingForYou,
                "Agent has a quick question before finishing",
                None,
                None,
            );
        }
        let rev = store.transition_task_state(
            &tid,
            TaskState::NeedsReview,
            "Demo commit ready",
            None,
            None,
        )?;
        if final_state == TaskState::Approved {
            return store.transition_task_state(
                &tid,
                TaskState::Approved,
                "Approved in sandbox",
                None,
                None,
            );
        }
        Ok(rev)
    };

    // 1. Clean approved task (ready to merge in Merge Queue!)
    let t1 = make_task(
        "Add bonuspollen multiplier",
        "Add bonusMultiplier helper to src/nectar.mjs while keeping nectarScore intact.",
        "src/nectar.mjs",
        "export function nectarScore(flowers) {\n  return flowers * 3;\n}\n\nexport function bonusMultiplier( hives ) {\n  return hives * 10;\n}\n",
        TaskState::Approved,
    )?;

    // Run real verification test on t1 so Tests summary shows 1 passed immediately!
    if let Ok(run) = run_test_command(
        &t1.id,
        Path::new(&t1.worktree_path),
        &test_command,
        logs_dir,
        Duration::from_secs(30),
        |_| {},
    )
    .await
    {
        let _ = store.insert_test_run(&run);
    }

    // 2. Clean NeedsReview docs task (ready for user to inspect Diff & click Approve!)
    let _t2 = make_task(
        "Document Hive quickstart guide",
        "Expand README.md with quickstart instructions and safety tips.",
        "README.md",
        "# Honeycomb Practice Repo\n\nWelcome to your safe Beehaven practice sandbox!\n\n## Quickstart\n1. Inspect Changes & Tests on any task.\n2. Check the Conflicts matrix before merging.\n3. Land approved branches through the Merge Queue.\n",
        TaskState::NeedsReview,
    )?;

    // 3 & 4. Deliberate conflicting edits on `src/banner.mjs` so Conflicts Matrix lights up!
    let _t3 = make_task(
        "Add seasonal golden banner",
        "Update bannerText() in src/banner.mjs with the autumn harvest greeting.",
        "src/banner.mjs",
        "export function bannerText() {\n  return 'Golden Harvest Edition — Honeycomb v2';\n}\n",
        TaskState::WaitingForYou,
    )?;

    let _t4 = make_task(
        "Localize welcome banner header",
        "Update bannerText() in src/banner.mjs to include hive status prefix.",
        "src/banner.mjs",
        "export function bannerText() {\n  return '[Hive Online] Welcome to Honeycomb!';\n}\n",
        TaskState::NeedsReview,
    )?;

    Ok(repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::conflict::{forecast_pair, MergeTreeOutcome};

    #[tokio::test]
    async fn demo_sandbox_creates_real_worktrees_and_detects_real_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let repo = create_demo_sandbox(tmp.path(), &tmp.path().join("logs"), &store)
            .await
            .unwrap();

        let tasks = store.list_tasks(&repo.id).unwrap();
        assert_eq!(tasks.len(), 4);

        // Tasks 3 and 4 touch `src/banner.mjs` on conflicting lines
        let outcome = forecast_pair(Path::new(&repo.path), &tasks[2].branch, &tasks[3].branch);
        match outcome {
            MergeTreeOutcome::Conflicted { files } => {
                assert_eq!(files, vec!["src/banner.mjs".to_string()]);
            }
            other => panic!("Expected real conflict in demo sandbox, got {other:?}"),
        }
    }
}
