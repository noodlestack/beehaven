use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::git::safety::{
    check_git_version, normalize_existing_path, run_git, slugify, validate_child_path,
};
use crate::models::{AgentInfo, RepoCheckResult, WorktreeInfo};

/// Computes the default sibling worktree root `<repo-parent>/<repo-name>.worktrees` (Section 4.4).
pub fn default_worktree_root(repo_path: &Path) -> AppResult<PathBuf> {
    let canon = normalize_existing_path(repo_path)?;
    let parent = canon.parent().ok_or_else(|| {
        AppError::user(
            "REPO_AT_FILESYSTEM_ROOT",
            "Repository cannot sit directly at the filesystem drive root.",
        )
    })?;
    let name = canon.file_name().and_then(|s| s.to_str()).unwrap_or("repo");
    Ok(parent.join(format!("{name}.worktrees")))
}

/// Suggests a test command by inspecting standard manifest files in the repository root.
pub fn suggest_test_command(repo_path: &Path) -> String {
    if repo_path.join("Cargo.toml").exists() {
        "cargo test".to_string()
    } else if repo_path.join("pnpm-lock.yaml").exists() {
        "pnpm test".to_string()
    } else if repo_path.join("yarn.lock").exists() {
        "yarn test".to_string()
    } else if repo_path.join("bun.lockb").exists() || repo_path.join("bun.lock").exists() {
        "bun test".to_string()
    } else if repo_path.join("package.json").exists() {
        "npm test".to_string()
    } else if repo_path.join("pyproject.toml").exists() || repo_path.join("pytest.ini").exists() {
        "pytest".to_string()
    } else if repo_path.join("go.mod").exists() {
        "go test ./...".to_string()
    } else {
        "echo \"No test command configured\"".to_string()
    }
}

/// Inspects a folder before or during `repo_open` (Section 3.4 / Section 4.6).
pub fn inspect_repo(path: &Path, detected_agents: Vec<AgentInfo>) -> AppResult<RepoCheckResult> {
    let (git_version, git_version_ok) = check_git_version()?;
    let canon = normalize_existing_path(path)?;

    let top = run_git(&canon, &["rev-parse", "--show-toplevel"])?;
    if !top.status.success() {
        return Ok(RepoCheckResult {
            path: canon.to_string_lossy().to_string(),
            is_git_repo: false,
            git_version,
            git_version_ok,
            detected_base_branch: "main".to_string(),
            available_branches: Vec::new(),
            suggested_test_command: suggest_test_command(&canon),
            detected_agents,
            clean_working_tree: true,
        });
    }

    let repo_root =
        normalize_existing_path(Path::new(String::from_utf8_lossy(&top.stdout).trim()))?;

    let branches_out = run_git(&repo_root, &["branch", "--format=%(refname:short)"])?;
    let available_branches: Vec<String> = String::from_utf8_lossy(&branches_out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let head_branch_out = run_git(&repo_root, &["symbolic-ref", "--short", "HEAD"])?;
    let head_branch = if head_branch_out.status.success() {
        Some(
            String::from_utf8_lossy(&head_branch_out.stdout)
                .trim()
                .to_string(),
        )
    } else {
        None
    };

    let detected_base_branch = if available_branches.iter().any(|b| b == "main") {
        "main".to_string()
    } else if available_branches.iter().any(|b| b == "master") {
        "master".to_string()
    } else if let Some(hb) = head_branch {
        hb
    } else if let Some(first) = available_branches.first() {
        first.clone()
    } else {
        "main".to_string()
    };

    let status_out = run_git(&repo_root, &["status", "--porcelain"])?;
    let clean_working_tree = status_out.status.success()
        && String::from_utf8_lossy(&status_out.stdout)
            .trim()
            .is_empty();

    Ok(RepoCheckResult {
        path: repo_root.to_string_lossy().to_string(),
        is_git_repo: true,
        git_version,
        git_version_ok,
        detected_base_branch,
        available_branches,
        suggested_test_command: suggest_test_command(&repo_root),
        detected_agents,
        clean_working_tree,
    })
}

/// Checks if a worktree directory has uncommitted tracked or untracked changes.
pub fn has_uncommitted_changes(worktree_path: &Path) -> AppResult<bool> {
    if !worktree_path.exists() {
        return Ok(false);
    }
    let out = run_git(worktree_path, &["status", "--porcelain"])?;
    if !out.status.success() {
        return Ok(false);
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Parses `git worktree list --porcelain` accurately, including paths with spaces.
pub fn list_worktrees(repo_path: &Path) -> AppResult<Vec<WorktreeInfo>> {
    let out = run_git(repo_path, &["worktree", "list", "--porcelain"])?;
    if !out.status.success() {
        return Err(AppError::with_details(
            "WORKTREE_LIST_FAILED",
            "Couldn't list worktrees for this repository.",
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
    }

    let raw = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    let mut current_path: Option<String> = None;
    let mut head_sha = String::new();
    let mut branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_detached = false;

    let flush = |path_opt: Option<String>,
                 head: String,
                 br: Option<String>,
                 bare: bool,
                 detached: bool,
                 acc: &mut Vec<WorktreeInfo>| {
        if let Some(p) = path_opt {
            let normalized = normalize_existing_path(Path::new(&p))
                .map(|pb| pb.to_string_lossy().to_string())
                .unwrap_or(p);
            let dirty = if bare {
                false
            } else {
                has_uncommitted_changes(Path::new(&normalized)).unwrap_or(false)
            };
            acc.push(WorktreeInfo {
                path: normalized,
                head_sha: head,
                branch: br,
                is_bare: bare,
                is_detached: detached,
                has_uncommitted_changes: dirty,
            });
        }
    };

    for line in raw.lines() {
        if line.trim().is_empty() {
            flush(
                current_path.take(),
                std::mem::take(&mut head_sha),
                branch.take(),
                is_bare,
                is_detached,
                &mut items,
            );
            is_bare = false;
            is_detached = false;
        } else if let Some(rest) = line.strip_prefix("worktree ") {
            current_path = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head_sha = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("branch ") {
            let short = rest.strip_prefix("refs/heads/").unwrap_or(rest).to_string();
            branch = Some(short);
        } else if line == "bare" {
            is_bare = true;
        } else if line == "detached" {
            is_detached = true;
        }
    }
    flush(
        current_path,
        head_sha,
        branch,
        is_bare,
        is_detached,
        &mut items,
    );

    Ok(items)
}

/// Creates a new isolated git worktree `<worktree_root>/<short_id>-<slug>` on branch `beehaven/<short_id>-<slug>`.
pub fn create_worktree(
    repo_path: &Path,
    worktree_root: &Path,
    base_branch: &str,
    task_id: &str,
    title_or_slug: &str,
) -> AppResult<(String, PathBuf)> {
    std::fs::create_dir_all(worktree_root)?;
    let canon_wt_root = normalize_existing_path(worktree_root)?;

    let short_id: String = task_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let slug = slugify(title_or_slug);
    let folder_name = if short_id.is_empty() {
        slug.clone()
    } else {
        format!("{short_id}-{slug}")
    };
    let branch_name = format!("beehaven/{folder_name}");
    let target_dir = validate_child_path(&canon_wt_root, Path::new(&folder_name))?;

    if target_dir.exists() {
        return Err(AppError::with_details(
            "WORKTREE_EXISTS",
            format!(
                "Couldn't create the worktree. A folder named '{}' already exists at that location.",
                folder_name
            ),
            target_dir.display().to_string(),
        ));
    }

    // Verify repo has at least one commit on base_branch
    let check_base = run_git(repo_path, &["rev-parse", "--verify", base_branch])?;
    if !check_base.status.success() {
        return Err(AppError::with_details(
            "BASE_BRANCH_MISSING",
            format!(
                "Base branch '{base_branch}' does not exist yet. Make at least one initial commit in the repository first."
            ),
            String::from_utf8_lossy(&check_base.stderr).to_string(),
        ));
    }

    let target_str = target_dir.to_string_lossy().to_string();
    let out = run_git(
        repo_path,
        &[
            "worktree",
            "add",
            "-b",
            &branch_name,
            &target_str,
            base_branch,
        ],
    )?;
    if !out.status.success() {
        return Err(AppError::with_details(
            "WORKTREE_ADD_FAILED",
            format!("Couldn't create worktree for branch '{branch_name}'."),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ));
    }

    let final_dir = normalize_existing_path(&target_dir)?;
    Ok((branch_name, final_dir))
}

/// Saves a complete `.patch` backup of both tracked modifications (`git diff HEAD`) AND
/// untracked files before any forced worktree removal or task discard (Section 3.1 / 4.4).
pub fn save_backup_patch(
    worktree_path: &Path,
    base_branch: Option<&str>,
    backup_dir: &Path,
    label: &str,
) -> AppResult<PathBuf> {
    std::fs::create_dir_all(backup_dir)?;
    let ts = crate::time_util::now_compact_utc();
    let safe_label = slugify(label);
    let patch_path = backup_dir.join(format!("{safe_label}-{ts}.patch"));

    let mut patch_buf = String::new();
    patch_buf.push_str(&format!(
        "# Beehaven Backup Patch\n# Label: {label}\n# Created: {ts}\n\n"
    ));

    // Include committed branch diff relative to base_branch if provided, plus uncommitted HEAD diff
    if let Some(base) = base_branch {
        if let Ok(mb) = run_git(worktree_path, &["merge-base", base, "HEAD"]) {
            if mb.status.success() {
                let sha = String::from_utf8_lossy(&mb.stdout).trim().to_string();
                if let Ok(diff_out) = run_git(worktree_path, &["diff", "--binary", &sha]) {
                    patch_buf.push_str(&String::from_utf8_lossy(&diff_out.stdout));
                }
            }
        }
    } else if let Ok(diff_out) = run_git(worktree_path, &["diff", "--binary", "HEAD"]) {
        patch_buf.push_str(&String::from_utf8_lossy(&diff_out.stdout));
    }

    // Also include untracked files so newly created agent files are never lost!
    if let Ok(untracked) = run_git(
        worktree_path,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    ) {
        if untracked.status.success() {
            for entry in untracked.stdout.split(|b| *b == 0) {
                if entry.is_empty() {
                    continue;
                }
                let rel = String::from_utf8_lossy(entry).to_string();
                let full = worktree_path.join(&rel);
                if let Ok(bytes) = std::fs::read(&full) {
                    if let Ok(text) = String::from_utf8(bytes) {
                        let line_count = text.lines().count();
                        patch_buf.push_str(&format!(
                            "\ndiff --git a/{rel} b/{rel}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{line_count} @@\n"
                        ));
                        for l in text.lines() {
                            patch_buf.push('+');
                            patch_buf.push_str(l);
                            patch_buf.push('\n');
                        }
                    }
                }
            }
        }
    }

    std::fs::write(&patch_path, patch_buf)?;
    Ok(patch_path)
}

/// Safely removes a worktree. Refuses if uncommitted changes exist unless `force = true`,
/// and always writes a backup `.patch` before removal when forced or discarding.
pub fn remove_worktree(
    repo_path: &Path,
    worktree_root: &Path,
    worktree_path: &Path,
    branch_to_delete: Option<&str>,
    base_branch: Option<&str>,
    force: bool,
    backup_dir: &Path,
) -> AppResult<Option<PathBuf>> {
    let safe_wt = validate_child_path(worktree_root, worktree_path)?;
    let canon_repo = normalize_existing_path(repo_path)?;
    if safe_wt == canon_repo {
        return Err(AppError::user(
            "CANNOT_REMOVE_MAIN_WORKTREE",
            "Refused to remove the main repository working directory.",
        ));
    }

    let mut saved_patch: Option<PathBuf> = None;

    if safe_wt.exists() {
        let dirty = has_uncommitted_changes(&safe_wt)?;
        if dirty && !force {
            return Err(AppError::user(
                "WORKTREE_DIRTY",
                "This worktree has uncommitted changes. Confirm discard to save a backup patch before removing it.",
            ));
        }

        let label = branch_to_delete.unwrap_or("worktree");
        saved_patch = Some(save_backup_patch(&safe_wt, base_branch, backup_dir, label)?);

        let wt_str = safe_wt.to_string_lossy().to_string();
        let args: Vec<&str> = if force {
            vec!["worktree", "remove", "--force", &wt_str]
        } else {
            vec!["worktree", "remove", &wt_str]
        };
        let out = run_git(&canon_repo, &args)?;
        if !out.status.success() {
            return Err(AppError::with_details(
                "WORKTREE_REMOVE_FAILED",
                "Couldn't remove the worktree folder. Make sure no external program has files locked inside it.",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
    } else {
        let _ = run_git(&canon_repo, &["worktree", "prune"]);
    }

    if let Some(branch) = branch_to_delete {
        if branch.starts_with("beehaven/") {
            let flag = if force { "-D" } else { "-d" };
            let _ = run_git(&canon_repo, &["branch", flag, branch]);
        }
    }

    Ok(saved_patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_temp_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Test User"]);
        run(&["config", "user.email", "test@example.com"]);
        fs::write(dir.join("README.md"), "# Sample\n").unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "Initial commit"]);
    }

    #[test]
    fn worktree_lifecycle_with_spaces_and_dirty_guard_and_backup_patch() {
        let parent = tempfile::tempdir().unwrap();
        let repo_dir = parent.path().join("My Repo With Spaces");
        fs::create_dir_all(&repo_dir).unwrap();
        init_temp_repo(&repo_dir);

        let check = inspect_repo(&repo_dir, vec![]).unwrap();
        assert!(check.is_git_repo);
        assert!(check.git_version_ok);
        assert_eq!(check.detected_base_branch, "main");
        assert_eq!(check.suggested_test_command, "cargo test");

        let wt_root = default_worktree_root(&repo_dir).unwrap();
        let backup_dir = parent.path().join("backups");

        let (b1, wt1) =
            create_worktree(&repo_dir, &wt_root, "main", "t1", "Add login validation").unwrap();
        let (_b2, wt2) =
            create_worktree(&repo_dir, &wt_root, "main", "t2", "Parser cleanup").unwrap();
        let (_b3, _wt3) =
            create_worktree(&repo_dir, &wt_root, "main", "t3", "Docs update").unwrap();

        let listed = list_worktrees(&repo_dir).unwrap();
        assert_eq!(listed.len(), 4); // main + 3 worktrees

        // Modify wt1 with uncommitted & untracked files
        fs::write(wt1.join("README.md"), "# Modified in wt1\n").unwrap();
        fs::write(wt1.join("new_feature.rs"), "pub fn hello() {}\n").unwrap();

        // Non-forced delete MUST refuse!
        let refused = remove_worktree(
            &repo_dir,
            &wt_root,
            &wt1,
            Some(&b1),
            Some("main"),
            false,
            &backup_dir,
        );
        assert!(refused.is_err());
        assert!(wt1.exists());

        // Forced delete MUST save a backup patch containing both modified and untracked content!
        let patch_opt = remove_worktree(
            &repo_dir,
            &wt_root,
            &wt1,
            Some(&b1),
            Some("main"),
            true,
            &backup_dir,
        )
        .unwrap();
        let patch_file = patch_opt.expect("backup patch must be created");
        assert!(patch_file.exists());
        let patch_text = fs::read_to_string(&patch_file).unwrap();
        assert!(patch_text.contains("# Modified in wt1"));
        assert!(patch_text.contains("new_feature.rs"));
        assert!(!wt1.exists());

        // Clean worktree deletes without force
        assert!(remove_worktree(
            &repo_dir,
            &wt_root,
            &wt2,
            None,
            Some("main"),
            false,
            &backup_dir
        )
        .is_ok());
    }
}
