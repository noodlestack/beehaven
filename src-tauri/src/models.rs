use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum TaskState {
    Queued,
    Running,
    WaitingForYou,
    NeedsReview,
    Approved,
    Merging,
    Merged,
    Failed,
    Discarded,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::WaitingForYou => "WaitingForYou",
            Self::NeedsReview => "NeedsReview",
            Self::Approved => "Approved",
            Self::Merging => "Merging",
            Self::Merged => "Merged",
            Self::Failed => "Failed",
            Self::Discarded => "Discarded",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "Queued" => Some(Self::Queued),
            "Running" => Some(Self::Running),
            "WaitingForYou" => Some(Self::WaitingForYou),
            "NeedsReview" => Some(Self::NeedsReview),
            "Approved" => Some(Self::Approved),
            "Merging" => Some(Self::Merging),
            "Merged" => Some(Self::Merged),
            "Failed" => Some(Self::Failed),
            "Discarded" => Some(Self::Discarded),
            _ => None,
        }
    }

    /// Enforces the exact state-machine transitions defined in Section 4.3 of the Build Plan,
    /// plus retry/re-review recovery paths from Failed so users are never dead-ended.
    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        if next == Self::Discarded && self != Self::Merged {
            return true;
        }
        matches!(
            (self, next),
            (Self::Queued, Self::Running)
                | (Self::Running, Self::WaitingForYou)
                | (Self::Running, Self::NeedsReview)
                | (Self::Running, Self::Failed)
                | (Self::WaitingForYou, Self::Running)
                | (Self::WaitingForYou, Self::NeedsReview)
                | (Self::WaitingForYou, Self::Failed)
                | (Self::NeedsReview, Self::Approved)
                | (Self::NeedsReview, Self::Running)
                | (Self::Approved, Self::Merging)
                | (Self::Approved, Self::NeedsReview)
                | (Self::Approved, Self::Running)
                | (Self::Merging, Self::Merged)
                | (Self::Merging, Self::Failed)
                | (Self::Merging, Self::Approved)
                | (Self::Failed, Self::Running)
                | (Self::Failed, Self::Queued)
                | (Self::Failed, Self::NeedsReview)
        )
    }

    pub fn mark(&self) -> &'static str {
        match self {
            Self::Queued => "○",
            Self::Running => "◐",
            Self::WaitingForYou => "▲",
            Self::NeedsReview => "◆",
            Self::Approved => "✓",
            Self::Merging => "⟳",
            Self::Merged => "✔",
            Self::Failed => "✗",
            Self::Discarded => "⊘",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::WaitingForYou => "Waiting for you",
            Self::NeedsReview => "Needs review",
            Self::Approved => "Approved",
            Self::Merging => "Merging",
            Self::Merged => "Merged",
            Self::Failed => "Failed",
            Self::Discarded => "Discarded",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Repo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub base_branch: String,
    pub test_command: String,
    pub worktree_root: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub supports_hooks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepoCheckResult {
    pub path: String,
    pub is_git_repo: bool,
    pub git_version: String,
    pub git_version_ok: bool,
    pub detected_base_branch: String,
    pub available_branches: Vec<String>,
    pub suggested_test_command: String,
    pub detected_agents: Vec<AgentInfo>,
    pub clean_working_tree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorktreeInfo {
    pub path: String,
    pub head_sha: String,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub has_uncommitted_changes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Task {
    pub id: String,
    pub repo_id: String,
    pub title: String,
    pub prompt: String,
    pub branch: String,
    pub worktree_path: String,
    pub agent: String,
    pub state: TaskState,
    pub backup_patch_path: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionRecord {
    pub id: String,
    pub task_id: String,
    pub pid: Option<u32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TestRun {
    pub id: String,
    pub task_id: String,
    pub commit_sha: String,
    pub passed: bool,
    pub log_path: String,
    pub output_preview: String,
    pub started_at: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiffFileStat {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiffSummary {
    pub task_id: String,
    pub base_branch: String,
    pub head_sha: String,
    pub total_additions: u32,
    pub total_deletions: u32,
    pub files: Vec<DiffFileStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiffLine {
    pub kind: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub hunks: Vec<DiffHunk>,
    pub raw_patch: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ConflictStatus {
    Clean,
    Conflicted,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConflictCell {
    pub task_a_id: String,
    pub task_b_id: String,
    pub sha_a: String,
    pub sha_b: String,
    pub status: ConflictStatus,
    pub files: Vec<String>,
    pub error_message: Option<String>,
    pub computed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeOrderSuggestion {
    pub ordered_task_ids: Vec<String>,
    pub reasoning: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConflictMatrixReport {
    pub repo_id: String,
    pub cells: Vec<ConflictCell>,
    pub suggestion: MergeOrderSuggestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum QueueItemStatus {
    Waiting,
    Rebasing,
    Testing,
    Merging,
    Merged,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueItem {
    pub task_id: String,
    pub title: String,
    pub branch: String,
    pub status: QueueItemStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueState {
    pub running: bool,
    pub paused: bool,
    pub stop_after_current: bool,
    pub items: Vec<QueueItem>,
    pub halted_reason: Option<String>,
    pub halted_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppSettings {
    pub max_concurrent_agents: usize,
    pub silence_timeout_secs: u64,
    pub theme: String,
    pub screen_reader_mode: bool,
    pub onboarding_completed: bool,
    pub default_agent: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            max_concurrent_agents: 4,
            silence_timeout_secs: 20,
            theme: "system".to_string(),
            screen_reader_mode: false,
            onboarding_completed: false,
            default_agent: "claude".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskEvent {
    pub id: i64,
    pub task_id: String,
    pub at: String,
    pub kind: String,
    pub detail_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionOutputPayload {
    pub task_id: String,
    pub chunk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaskStatePayload {
    pub task: Task,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TestsProgressPayload {
    pub task_id: String,
    pub chunk: String,
}

#[cfg(test)]
mod tests {
    use super::TaskState;

    #[test]
    fn enforces_nine_state_machine_transitions() {
        assert!(TaskState::Queued.can_transition_to(TaskState::Running));
        assert!(!TaskState::Queued.can_transition_to(TaskState::Merged));
        assert!(!TaskState::Queued.can_transition_to(TaskState::Approved));

        assert!(TaskState::Running.can_transition_to(TaskState::WaitingForYou));
        assert!(TaskState::Running.can_transition_to(TaskState::NeedsReview));
        assert!(TaskState::Running.can_transition_to(TaskState::Failed));
        assert!(!TaskState::Running.can_transition_to(TaskState::Merged));

        assert!(TaskState::WaitingForYou.can_transition_to(TaskState::Running));
        assert!(TaskState::NeedsReview.can_transition_to(TaskState::Approved));
        assert!(TaskState::NeedsReview.can_transition_to(TaskState::Running));
        assert!(TaskState::NeedsReview.can_transition_to(TaskState::Discarded));

        assert!(TaskState::Approved.can_transition_to(TaskState::Merging));
        assert!(TaskState::Merging.can_transition_to(TaskState::Merged));
        assert!(TaskState::Merging.can_transition_to(TaskState::Failed));

        // Anything except Merged can be discarded
        assert!(TaskState::Queued.can_transition_to(TaskState::Discarded));
        assert!(TaskState::Running.can_transition_to(TaskState::Discarded));
        assert!(!TaskState::Merged.can_transition_to(TaskState::Discarded));
    }
}
