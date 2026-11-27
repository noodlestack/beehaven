use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use crate::error::{AppError, AppResult};

/// Strict allowlist of git subcommands Beehaven is permitted to run (Section 4.4).
const ALLOWED_SUBCOMMANDS: &[&str] = &[
    "--version",
    "version",
    "init",
    "config",
    "rev-parse",
    "status",
    "branch",
    "symbolic-ref",
    "worktree",
    "diff",
    "ls-files",
    "show",
    "log",
    "merge-base",
    "merge-tree",
    "merge",
    "rebase",
    "checkout",
    "switch",
    "update-ref",
    "commit",
    "add",
    "check-ref-format",
];

/// Strips Windows `\\?\` extended-length prefix produced by `std::fs::canonicalize`
/// so external CLI tools (`git`, shells, Node, Claude Code) receive standard paths.
pub fn strip_windows_unc(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = path.to_str() {
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
        }
    }
    path
}

/// Canonicalizes an existing path on disk and strips `\\?\` on Windows.
pub fn normalize_existing_path(path: &Path) -> AppResult<PathBuf> {
    let canon = std::fs::canonicalize(path).map_err(|e| {
        AppError::with_details(
            "INVALID_PATH",
            format!(
                "Path does not exist or cannot be accessed: {}",
                path.display()
            ),
            e.to_string(),
        )
    })?;
    Ok(strip_windows_unc(canon))
}

/// Validates that `candidate` resolves inside `allowed_root`, even if the leaf segment(s)
/// of `candidate` have not been created yet. Rejects `..` parent traversal and root escapes.
pub fn validate_child_path(allowed_root: &Path, candidate: &Path) -> AppResult<PathBuf> {
    let canon_root = normalize_existing_path(allowed_root)?;

    for comp in candidate.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(AppError::user(
                "PATH_TRAVERSAL",
                "Path cannot contain parent directory ('..') segments.",
            ));
        }
    }

    // Walk up until we find an existing ancestor to canonicalize, then re-append tail segments.
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        canon_root.join(candidate)
    };

    let mut existing = joined.as_path();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        if let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) {
            tail.push(name.to_os_string());
            existing = parent;
        } else {
            return Err(AppError::user(
                "INVALID_PATH",
                format!("Cannot resolve ancestor for {}", joined.display()),
            ));
        }
    }

    let mut resolved = normalize_existing_path(existing)?;
    for part in tail.into_iter().rev() {
        resolved.push(part);
    }

    if !resolved.starts_with(&canon_root) {
        return Err(AppError::with_details(
            "PATH_OUTSIDE_ROOT",
            "Refused access to a path outside the repository or worktree root.",
            format!(
                "Resolved '{}' is outside '{}'",
                resolved.display(),
                canon_root.display()
            ),
        ));
    }

    Ok(resolved)
}

/// Converts a task title into a safe lowercase ASCII slug for folder and branch names.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 36 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed
    }
}

/// Validates git arguments against Beehaven's subcommand allowlist and destructive-flag blocklist.
pub fn validate_git_args(args: &[&str]) -> AppResult<()> {
    let Some(&sub) = args.first() else {
        return Err(AppError::user("GIT_ARGS_EMPTY", "Empty git command."));
    };

    if !ALLOWED_SUBCOMMANDS.contains(&sub) {
        return Err(AppError::with_details(
            "GIT_FORBIDDEN_SUBCOMMAND",
            format!("Git subcommand '{sub}' is not permitted by Beehaven safety guards."),
            format!("Args: {args:?}"),
        ));
    }

    // Explicitly forbid destructive flags (Section 4.4 / Rule 3)
    if sub == "clean" || sub == "push" {
        return Err(AppError::user(
            "GIT_FORBIDDEN_DESTRUCTIVE",
            format!("Git '{sub}' is forbidden by Beehaven safety guards."),
        ));
    }
    if sub == "reset" && args.contains(&"--hard") {
        return Err(AppError::user(
            "GIT_FORBIDDEN_RESET_HARD",
            "git reset --hard is forbidden by Beehaven safety guards.",
        ));
    }
    if (sub == "checkout" || sub == "switch") && args.iter().any(|a| *a == "-f" || *a == "--force")
    {
        return Err(AppError::user(
            "GIT_FORBIDDEN_FORCE_CHECKOUT",
            "Forced checkout is forbidden by Beehaven safety guards.",
        ));
    }

    Ok(())
}

/// Runs `git` inside `cwd` with safety validation, non-interactive env, and UTF-8 paths.
pub fn run_git(cwd: &Path, args: &[&str]) -> AppResult<Output> {
    validate_git_args(args)?;
    let safe_cwd = normalize_existing_path(cwd)?;

    let mut cmd = Command::new("git");
    cmd.current_dir(&safe_cwd)
        .arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C.UTF-8");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd.output().map_err(|e| {
        AppError::with_details(
            "GIT_EXEC_FAILED",
            "Couldn't run git. Make sure Git 2.38 or newer is installed and on your PATH.",
            e.to_string(),
        )
    })
}

/// Parses `git --version` and verifies `major > 2 || (major == 2 && minor >= 38)`.
pub fn check_git_version() -> AppResult<(String, bool)> {
    let mut cmd = Command::new("git");
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| {
        AppError::with_details(
            "GIT_MISSING",
            "Git is not installed or not on your PATH. Install Git 2.38 or newer.",
            e.to_string(),
        )
    })?;
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let ok = parse_git_version_at_least_2_38(&raw);
    Ok((raw, ok))
}

pub fn parse_git_version_at_least_2_38(version_output: &str) -> bool {
    // e.g. "git version 2.50.1.windows.1" or "git version 2.39.3 (Apple Git-146)"
    for token in version_output.split_whitespace() {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() >= 2 {
            if let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                return major > 2 || (major == 2 && minor >= 38);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_versions_accurately() {
        assert!(parse_git_version_at_least_2_38(
            "git version 2.50.1.windows.1"
        ));
        assert!(parse_git_version_at_least_2_38("git version 2.38.0"));
        assert!(parse_git_version_at_least_2_38("git version 3.0.0"));
        assert!(!parse_git_version_at_least_2_38("git version 2.37.4"));
        assert!(!parse_git_version_at_least_2_38("git version 1.9.5"));
    }

    #[test]
    fn blocks_forbidden_git_commands_and_flags() {
        assert!(validate_git_args(&["clean", "-fd"]).is_err());
        assert!(validate_git_args(&["push", "--force"]).is_err());
        assert!(validate_git_args(&["push", "origin", "main"]).is_err());
        assert!(validate_git_args(&["reset", "--hard", "HEAD"]).is_err());
        assert!(validate_git_args(&["checkout", "-f", "main"]).is_err());
        assert!(validate_git_args(&["worktree", "list", "--porcelain"]).is_ok());
        assert!(validate_git_args(&["merge-tree", "--write-tree", "-z", "a", "b"]).is_ok());
    }

    #[test]
    fn blocks_path_traversal_outside_root() {
        let tmp = tempfile::tempdir().unwrap();
        let inside = validate_child_path(tmp.path(), Path::new("task-1")).unwrap();
        assert!(inside.starts_with(normalize_existing_path(tmp.path()).unwrap()));
        assert!(validate_child_path(tmp.path(), Path::new("../escape")).is_err());
    }
}
