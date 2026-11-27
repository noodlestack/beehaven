use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::git::safety::{normalize_existing_path, run_git, validate_child_path};
use crate::models::{DiffFileStat, DiffHunk, DiffLine, DiffSummary, FileDiff};

const MAX_RENDERED_DIFF_LINES: usize = 8_000;

fn resolve_merge_base(worktree_path: &Path, base_branch: &str) -> AppResult<String> {
    let mb = run_git(worktree_path, &["merge-base", base_branch, "HEAD"])?;
    if mb.status.success() {
        let sha = String::from_utf8_lossy(&mb.stdout).trim().to_string();
        if !sha.is_empty() {
            return Ok(sha);
        }
    }
    let rev = run_git(worktree_path, &["rev-parse", base_branch])?;
    if !rev.status.success() {
        return Err(AppError::with_details(
            "MERGE_BASE_FAILED",
            format!("Couldn't resolve base branch '{base_branch}' for diff."),
            String::from_utf8_lossy(&rev.stderr).to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&rev.stdout).trim().to_string())
}

/// Computes file-level diff statistics against `merge-base(base_branch, HEAD)` including
/// both committed branch changes and uncommitted/untracked working tree changes.
pub fn diff_summary(
    task_id: &str,
    worktree_path: &Path,
    base_branch: &str,
) -> AppResult<DiffSummary> {
    let canon_wt = normalize_existing_path(worktree_path)?;
    let merge_base = resolve_merge_base(&canon_wt, base_branch)?;

    let head_out = run_git(&canon_wt, &["rev-parse", "HEAD"])?;
    let head_sha = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

    let mut status_map: BTreeMap<String, String> = BTreeMap::new();
    let name_status = run_git(&canon_wt, &["diff", "--name-status", &merge_base])?;
    for line in String::from_utf8_lossy(&name_status.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let code = parts[0].chars().next().unwrap_or('M').to_string();
            let path = parts.last().copied().unwrap_or("").to_string();
            if !path.is_empty() {
                status_map.insert(path, code);
            }
        }
    }

    let mut files_map: BTreeMap<String, DiffFileStat> = BTreeMap::new();
    let numstat = run_git(&canon_wt, &["diff", "--numstat", &merge_base])?;
    for line in String::from_utf8_lossy(&numstat.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let adds = parts[0].parse::<u32>().unwrap_or(0);
            let dels = parts[1].parse::<u32>().unwrap_or(0);
            let path = parts[2..].join("\t");
            let status = status_map
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "M".to_string());
            files_map.insert(
                path.clone(),
                DiffFileStat {
                    path,
                    status,
                    additions: adds,
                    deletions: dels,
                },
            );
        }
    }

    // Include untracked files so work-in-progress files show up cleanly in Changes tab!
    let untracked = run_git(
        &canon_wt,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    if untracked.status.success() {
        for raw in untracked.stdout.split(|b| *b == 0) {
            if raw.is_empty() {
                continue;
            }
            let rel = String::from_utf8_lossy(raw).to_string();
            if files_map.contains_key(&rel) {
                continue;
            }
            let full = canon_wt.join(&rel);
            let adds = std::fs::read_to_string(&full)
                .map(|s| s.lines().count() as u32)
                .unwrap_or(0);
            files_map.insert(
                rel.clone(),
                DiffFileStat {
                    path: rel,
                    status: "A".to_string(),
                    additions: adds,
                    deletions: 0,
                },
            );
        }
    }

    let files: Vec<DiffFileStat> = files_map.into_values().collect();
    let total_additions = files.iter().map(|f| f.additions).sum();
    let total_deletions = files.iter().map(|f| f.deletions).sum();

    Ok(DiffSummary {
        task_id: task_id.to_string(),
        base_branch: base_branch.to_string(),
        head_sha,
        total_additions,
        total_deletions,
        files,
    })
}

/// Lazily loads and parses hunks for a single file inside `worktree_path`.
pub fn file_diff(worktree_path: &Path, base_branch: &str, rel_path: &str) -> AppResult<FileDiff> {
    let canon_wt = normalize_existing_path(worktree_path)?;
    let full_target = validate_child_path(&canon_wt, Path::new(rel_path))?;
    let merge_base = resolve_merge_base(&canon_wt, base_branch)?;

    let out = run_git(
        &canon_wt,
        &["diff", "--unified=3", &merge_base, "--", rel_path],
    )?;
    let mut raw_patch = String::from_utf8_lossy(&out.stdout).to_string();
    let mut status = "M".to_string();

    if raw_patch.trim().is_empty() && full_target.exists() {
        // Untracked new file
        status = "A".to_string();
        if let Ok(bytes) = std::fs::read(&full_target) {
            match String::from_utf8(bytes) {
                Ok(content) => {
                    let count = content.lines().count();
                    let mut synth = format!(
                        "diff --git a/{rel_path} b/{rel_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel_path}\n@@ -0,0 +1,{count} @@\n"
                    );
                    for l in content.lines() {
                        synth.push('+');
                        synth.push_str(l);
                        synth.push('\n');
                    }
                    raw_patch = synth;
                }
                Err(_) => {
                    raw_patch = format!("Binary file {rel_path} added.\n");
                }
            }
        }
    } else if raw_patch.contains("\nnew file mode ") {
        status = "A".to_string();
    } else if raw_patch.contains("\ndeleted file mode ") {
        status = "D".to_string();
    }

    Ok(parse_unified_patch(rel_path, &status, &raw_patch))
}

pub fn parse_unified_patch(rel_path: &str, status: &str, raw_patch: &str) -> FileDiff {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line: u32 = 0;
    let mut new_line: u32 = 0;
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;
    let mut rendered_lines: usize = 0;
    let mut truncated = false;

    for line in raw_patch.lines() {
        if line.starts_with("@@ ") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            let (os, ol, ns, nl) = parse_hunk_header(line);
            old_line = os;
            new_line = ns;
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                old_start: os,
                old_lines: ol,
                new_start: ns,
                new_lines: nl,
                lines: Vec::new(),
            });
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            continue;
        };

        if rendered_lines >= MAX_RENDERED_DIFF_LINES {
            truncated = true;
            if let Some(stripped) = line.strip_prefix('+') {
                let _ = stripped;
                additions += 1;
            } else if line.starts_with('-') {
                deletions += 1;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix('+') {
            additions += 1;
            hunk.lines.push(DiffLine {
                kind: "add".to_string(),
                old_lineno: None,
                new_lineno: Some(new_line),
                content: rest.to_string(),
            });
            new_line += 1;
            rendered_lines += 1;
        } else if let Some(rest) = line.strip_prefix('-') {
            deletions += 1;
            hunk.lines.push(DiffLine {
                kind: "del".to_string(),
                old_lineno: Some(old_line),
                new_lineno: None,
                content: rest.to_string(),
            });
            old_line += 1;
            rendered_lines += 1;
        } else if let Some(rest) = line.strip_prefix(' ') {
            hunk.lines.push(DiffLine {
                kind: "context".to_string(),
                old_lineno: Some(old_line),
                new_lineno: Some(new_line),
                content: rest.to_string(),
            });
            old_line += 1;
            new_line += 1;
            rendered_lines += 1;
        }
    }

    if let Some(h) = current_hunk {
        hunks.push(h);
    }

    let capped_raw = if truncated {
        raw_patch
            .lines()
            .take(MAX_RENDERED_DIFF_LINES)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        raw_patch.to_string()
    };

    FileDiff {
        path: rel_path.to_string(),
        status: status.to_string(),
        additions,
        deletions,
        hunks,
        raw_patch: capped_raw,
        truncated,
    }
}

fn parse_hunk_header(header: &str) -> (u32, u32, u32, u32) {
    // e.g. "@@ -10,5 +12,8 @@ fn demo()"
    let mut old_start = 1;
    let mut old_lines = 0;
    let mut new_start = 1;
    let mut new_lines = 0;

    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() >= 3 {
        if let Some(old_spec) = parts[1].strip_prefix('-') {
            let (s, l) = parse_range(old_spec);
            old_start = s;
            old_lines = l;
        }
        if let Some(new_spec) = parts[2].strip_prefix('+') {
            let (s, l) = parse_range(new_spec);
            new_start = s;
            new_lines = l;
        }
    }
    (old_start, old_lines, new_start, new_lines)
}

fn parse_range(spec: &str) -> (u32, u32) {
    if let Some((start, count)) = spec.split_once(',') {
        (start.parse().unwrap_or(1), count.parse().unwrap_or(0))
    } else {
        (spec.parse().unwrap_or(1), 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unified_git_patch_accurately() {
        let sample = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 pub fn greet() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"second\");
 }
";
        let parsed = parse_unified_patch("src/lib.rs", "M", sample);
        assert_eq!(parsed.additions, 2);
        assert_eq!(parsed.deletions, 1);
        assert_eq!(parsed.hunks.len(), 1);
        assert_eq!(parsed.hunks[0].lines.len(), 5);
        assert_eq!(parsed.hunks[0].lines[1].kind, "del");
        assert_eq!(parsed.hunks[0].lines[2].kind, "add");
    }
}
