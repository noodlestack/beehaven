export type TaskState =
  | "Queued"
  | "Running"
  | "WaitingForYou"
  | "NeedsReview"
  | "Approved"
  | "Merging"
  | "Merged"
  | "Failed"
  | "Discarded";

export interface AppErrorPayload {
  code: string;
  message: string;
  details: string | null;
}

export interface Repo {
  id: string;
  name: string;
  path: string;
  base_branch: string;
  test_command: string;
  worktree_root: string;
  created_at: string;
}

export interface AgentInfo {
  id: string;
  name: string;
  installed: boolean;
  version: string | null;
  supports_hooks: boolean;
}

export interface RepoCheckResult {
  path: string;
  is_git_repo: boolean;
  git_version: string;
  git_version_ok: boolean;
  detected_base_branch: string;
  available_branches: string[];
  suggested_test_command: string;
  detected_agents: AgentInfo[];
  clean_working_tree: boolean;
}

export interface WorktreeInfo {
  path: string;
  head_sha: string;
  branch: string | null;
  is_bare: boolean;
  is_detached: boolean;
  has_uncommitted_changes: boolean;
}

export interface Task {
  id: string;
  repo_id: string;
  title: string;
  prompt: string;
  branch: string;
  worktree_path: string;
  agent: string;
  state: TaskState;
  backup_patch_path: string | null;
  last_error: string | null;
  created_at: string;
  updated_at: string;
}

export interface TestRun {
  id: string;
  task_id: string;
  commit_sha: string;
  passed: boolean;
  log_path: string;
  output_preview: string;
  started_at: string;
  duration_ms: number;
}

export interface DiffFileStat {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

export interface DiffSummary {
  task_id: string;
  base_branch: string;
  head_sha: string;
  total_additions: number;
  total_deletions: number;
  files: DiffFileStat[];
}

export interface DiffLine {
  kind: "context" | "add" | "del" | string;
  old_lineno: number | null;
  new_lineno: number | null;
  content: string;
}

export interface DiffHunk {
  header: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  hunks: DiffHunk[];
  raw_patch: string;
  truncated: boolean;
}

export type ConflictStatus = "Clean" | "Conflicted" | "Error";

export interface ConflictCell {
  task_a_id: string;
  task_b_id: string;
  sha_a: string;
  sha_b: string;
  status: ConflictStatus;
  files: string[];
  error_message: string | null;
  computed_at: string;
}

export interface MergeOrderSuggestion {
  ordered_task_ids: string[];
  reasoning: string[];
}

export interface ConflictMatrixReport {
  repo_id: string;
  cells: ConflictCell[];
  suggestion: MergeOrderSuggestion;
}

export type QueueItemStatus =
  | "Waiting"
  | "Rebasing"
  | "Testing"
  | "Merging"
  | "Merged"
  | "Failed"
  | "Stopped";

export interface QueueItem {
  task_id: string;
  title: string;
  branch: string;
  status: QueueItemStatus;
  detail: string;
}

export interface QueueState {
  running: boolean;
  paused: boolean;
  stop_after_current: boolean;
  items: QueueItem[];
  halted_reason: string | null;
  halted_task_id: string | null;
}

export interface AppSettings {
  max_concurrent_agents: number;
  silence_timeout_secs: number;
  theme: "system" | "dark" | "light" | string;
  screen_reader_mode: boolean;
  onboarding_completed: boolean;
  default_agent: string;
}

export interface TaskEvent {
  id: number;
  task_id: string;
  at: string;
  kind: string;
  detail_json: string;
}

export interface SessionOutputPayload {
  task_id: string;
  chunk: string;
}

export interface TaskStatePayload {
  task: Task;
  reason: string;
}

export interface TestsProgressPayload {
  task_id: string;
  chunk: string;
}
