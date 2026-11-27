use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::git::safety::run_git;
use crate::models::{ConflictCell, ConflictStatus, MergeOrderSuggestion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeTreeOutcome {
    Clean,
    Conflicted { files: Vec<String> },
    Error { message: String },
}

/// Runs `git merge-tree --write-tree -z <branch_a> <branch_b>` and classifies the outcome
/// strictly by exit status (Section 4.4 / S0.C / S5):
/// - Exit 0 -> Clean
/// - Exit 1 -> Conflicted (parses NUL-delimited Conflicted file info section)
/// - Anything else -> Error (NEVER treated as Clean!)
pub fn forecast_pair(repo_path: &Path, ref_a: &str, ref_b: &str) -> MergeTreeOutcome {
    let out = match run_git(
        repo_path,
        &["merge-tree", "--write-tree", "-z", ref_a, ref_b],
    ) {
        Ok(o) => o,
        Err(e) => {
            return MergeTreeOutcome::Error {
                message: e.to_string(),
            }
        }
    };

    match out.status.code() {
        Some(0) => MergeTreeOutcome::Clean,
        Some(1) => {
            // Verify stdout starts with a valid hex tree OID before `\0`.
            // When `git merge-tree` fails to run (e.g. invalid ref on Git 2.50 Windows),
            // it exits with code 1, empty stdout, and an error on stderr!
            let first_field = out.stdout.split(|b| *b == 0).next().unwrap_or(&[]);
            let has_valid_oid =
                first_field.len() >= 40 && first_field.iter().all(|b| b.is_ascii_hexdigit());
            if !has_valid_oid {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let msg = if !stderr.is_empty() {
                    stderr
                } else {
                    "git merge-tree could not merge the specified refs".to_string()
                };
                return MergeTreeOutcome::Error { message: msg };
            }
            let mut files = parse_merge_tree_z_conflicts(&out.stdout);
            if files.is_empty() {
                files.push("(tree conflict)".to_string());
            }
            MergeTreeOutcome::Conflicted { files }
        }
        other => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let msg = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("git merge-tree failed with status {other:?}")
            };
            MergeTreeOutcome::Error { message: msg }
        }
    }
}

/// Parses the machine-readable NUL-separated output of `git merge-tree --write-tree -z`:
/// `<tree-oid>\0<mode> <oid> <stage>\t<filename>\0...\0<informational-messages>`
pub fn parse_merge_tree_z_conflicts(stdout: &[u8]) -> Vec<String> {
    let mut files = BTreeSet::new();
    let mut parts = stdout.split(|b| *b == 0);

    // First NUL-terminated field is the top-level tree OID.
    let _tree_oid = parts.next();

    // Subsequent fields up to an empty NUL-terminated field (`\0\0`) are Conflicted file info:
    // `<mode> <object> <stage>\t<filename>`
    for entry in parts {
        if entry.is_empty() {
            // End of Conflicted file info section; informational messages follow.
            break;
        }
        let text = String::from_utf8_lossy(entry);
        if let Some((_meta, filename)) = text.split_once('\t') {
            let trimmed = filename.trim();
            if !trimmed.is_empty() {
                files.insert(trimmed.to_string());
            }
        }
    }

    files.into_iter().collect()
}

#[derive(Debug, Clone)]
pub struct CandidateBranchMetric {
    pub task_id: String,
    pub index_label: String,
    pub title: String,
    pub changed_lines: u32,
}

/// Computes a greedy suggested merge order (Section 3.2 & S5):
/// Fewest active conflicts first, breaking ties by smaller diff (`changed_lines`),
/// and explains which downstream tasks will need a rebase after an earlier task merges.
pub fn suggest_merge_order(
    tasks: &[CandidateBranchMetric],
    cells: &[ConflictCell],
) -> MergeOrderSuggestion {
    if tasks.is_empty() {
        return MergeOrderSuggestion {
            ordered_task_ids: Vec::new(),
            reasoning: vec!["No active task branches to order yet.".to_string()],
        };
    }

    let mut conflict_neighbors: HashMap<String, HashSet<String>> = HashMap::new();
    for t in tasks {
        conflict_neighbors.insert(t.task_id.clone(), HashSet::new());
    }

    for c in cells {
        if c.status == ConflictStatus::Conflicted || c.status == ConflictStatus::Error {
            if let Some(set) = conflict_neighbors.get_mut(&c.task_a_id) {
                set.insert(c.task_b_id.clone());
            }
            if let Some(set) = conflict_neighbors.get_mut(&c.task_b_id) {
                set.insert(c.task_a_id.clone());
            }
        }
    }

    let mut sorted = tasks.to_vec();
    sorted.sort_by(|a, b| {
        let ca = conflict_neighbors
            .get(&a.task_id)
            .map(|s| s.len())
            .unwrap_or(0);
        let cb = conflict_neighbors
            .get(&b.task_id)
            .map(|s| s.len())
            .unwrap_or(0);
        ca.cmp(&cb)
            .then_with(|| a.changed_lines.cmp(&b.changed_lines))
            .then_with(|| a.index_label.cmp(&b.index_label))
    });

    let ordered_task_ids: Vec<String> = sorted.iter().map(|t| t.task_id.clone()).collect();
    let by_id: HashMap<String, &CandidateBranchMetric> =
        tasks.iter().map(|t| (t.task_id.clone(), t)).collect();

    let mut reasoning = Vec::new();
    let mut merged_so_far: Vec<String> = Vec::new();

    for item in &sorted {
        let neighbors = conflict_neighbors
            .get(&item.task_id)
            .cloned()
            .unwrap_or_default();
        let earlier_conflicts: Vec<String> = merged_so_far
            .iter()
            .filter(|prev_id| neighbors.contains(*prev_id))
            .filter_map(|prev_id| by_id.get(prev_id).map(|m| m.title.clone()))
            .collect();

        if neighbors.is_empty() {
            reasoning.push(format!(
                "'{}' has 0 conflicts ({} changed lines) — merge cleanly first.",
                item.title, item.changed_lines
            ));
        } else if earlier_conflicts.is_empty() {
            reasoning.push(format!(
                "'{}' conflicts with {} later branch(es) ({} changed lines) — merge before conflicting peers.",
                item.title,
                neighbors.len(),
                item.changed_lines
            ));
        } else {
            reasoning.push(format!(
                "'{}' will need a conflict resolution rebase after {}.",
                item.title,
                earlier_conflicts.join(", ")
            ));
        }
        merged_so_far.push(item.task_id.clone());
    }

    MergeOrderSuggestion {
        ordered_task_ids,
        reasoning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn merge_tree_spike_detects_clean_conflicted_and_error_outcomes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "user.name", "Tester"]);
        git(repo, &["config", "user.email", "tester@example.com"]);

        fs::write(repo.join("shared.txt"), "line 1\nline 2\n").unwrap();
        fs::write(repo.join("docs.md"), "initial docs\n").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "base"]);

        // Branch A modifies shared.txt
        git(repo, &["checkout", "-b", "branch-a"]);
        fs::write(repo.join("shared.txt"), "branch A edit\nline 2\n").unwrap();
        git(repo, &["commit", "-am", "edit shared on A"]);

        // Branch B modifies shared.txt differently -> CONFLICT!
        git(repo, &["checkout", "main"]);
        git(repo, &["checkout", "-b", "branch-b"]);
        fs::write(
            repo.join("shared.txt"),
            "branch B conflicting edit\nline 2\n",
        )
        .unwrap();
        git(repo, &["commit", "-am", "edit shared on B"]);

        // Branch C modifies docs.md -> CLEAN against both A and B!
        git(repo, &["checkout", "main"]);
        git(repo, &["checkout", "-b", "branch-c"]);
        fs::write(repo.join("docs.md"), "updated docs on C\n").unwrap();
        git(repo, &["commit", "-am", "edit docs on C"]);

        assert_eq!(
            forecast_pair(repo, "branch-a", "branch-c"),
            MergeTreeOutcome::Clean
        );
        assert_eq!(
            forecast_pair(repo, "branch-b", "branch-c"),
            MergeTreeOutcome::Clean
        );

        match forecast_pair(repo, "branch-a", "branch-b") {
            MergeTreeOutcome::Conflicted { files } => {
                assert_eq!(files, vec!["shared.txt".to_string()]);
            }
            other => panic!("Expected Conflicted, got {other:?}"),
        }

        // Non-existent ref MUST return Error, NEVER Clean!
        match forecast_pair(repo, "branch-a", "non-existent-branch-xyz") {
            MergeTreeOutcome::Error { .. } => {}
            other => panic!("Expected Error for invalid ref, got {other:?}"),
        }
    }
}
