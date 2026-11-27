import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentInfo,
  AppErrorPayload,
  AppSettings,
  ConflictMatrixReport,
  DiffSummary,
  FileDiff,
  QueueState,
  Repo,
  RepoCheckResult,
  Task,
  TaskEvent,
  TaskState,
  TestRun,
  WorktreeInfo,
} from "../types/models";

export function isTauriRuntime(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

// ============================================================================
// Rich browser/test fallback store so web previews and Vitest suites run smoothly
// ============================================================================
interface MockBackendState {
  repos: Repo[];
  tasks: Task[];
  settings: AppSettings;
  testRuns: Record<string, TestRun[]>;
  terminalReplay: Record<string, string>;
  queue: QueueState;
  listeners: Record<string, Array<(payload: unknown) => void>>;
}

const nowIso = () => new Date().toISOString();

const mockState: MockBackendState = {
  repos: [
    {
      id: "demo-repo-1",
      name: "my-app",
      path: "C:/projects/my-app",
      base_branch: "main",
      test_command: "cargo test",
      worktree_root: "C:/projects/my-app.worktrees",
      created_at: "2026-11-27T12:00:00Z",
    },
  ],
  tasks: [
    {
      id: "task-1",
      repo_id: "demo-repo-1",
      title: "Parser streaming support",
      prompt: "Add streaming chunk parser with unit tests",
      branch: "beehaven/task1-parser-streaming",
      worktree_path: "C:/projects/my-app.worktrees/task1-parser-streaming",
      agent: "claude",
      state: "Running",
      backup_patch_path: null,
      last_error: null,
      created_at: "2026-11-27T12:05:00Z",
      updated_at: "2026-11-27T12:12:00Z",
    },
    {
      id: "task-2",
      repo_id: "demo-repo-1",
      title: "Refactor auth middleware",
      prompt: "Extract token verification into src/auth/verify.rs",
      branch: "beehaven/task2-refactor-auth",
      worktree_path: "C:/projects/my-app.worktrees/task2-refactor-auth",
      agent: "claude",
      state: "Approved",
      backup_patch_path: null,
      last_error: null,
      created_at: "2026-11-27T12:06:00Z",
      updated_at: "2026-11-27T12:18:00Z",
    },
    {
      id: "task-3",
      repo_id: "demo-repo-1",
      title: "Add login validation",
      prompt: "Validate email & password rules on /login endpoint",
      branch: "beehaven/task3-login-validation",
      worktree_path: "C:/projects/my-app.worktrees/task3-login-validation",
      agent: "claude",
      state: "WaitingForYou",
      backup_patch_path: null,
      last_error: null,
      created_at: "2026-11-27T12:07:00Z",
      updated_at: "2026-11-27T12:20:00Z",
    },
    {
      id: "task-4",
      repo_id: "demo-repo-1",
      title: "Update API docs",
      prompt: "Document error codes and rate limits in docs/API.md",
      branch: "beehaven/task4-docs",
      worktree_path: "C:/projects/my-app.worktrees/task4-docs",
      agent: "generic",
      state: "NeedsReview",
      backup_patch_path: null,
      last_error: null,
      created_at: "2026-11-27T12:08:00Z",
      updated_at: "2026-11-27T12:22:00Z",
    },
  ],
  settings: {
    max_concurrent_agents: 4,
    silence_timeout_secs: 20,
    theme: "dark",
    screen_reader_mode: false,
    onboarding_completed: true,
    default_agent: "claude",
  },
  testRuns: {
    "task-2": [
      {
        id: "tr-1",
        task_id: "task-2",
        commit_sha: "a91f4c82",
        passed: true,
        log_path: "C:/beehaven/logs/test-task-2.log",
        output_preview: "running 14 tests\ntest auth::verify::validates_jwt ... ok\ntest result: ok. 14 passed; 0 failed; finished in 0.42s\n",
        started_at: "2026-11-27T12:17:00Z",
        duration_ms: 420,
      },
    ],
    "task-4": [
      {
        id: "tr-2",
        task_id: "task-4",
        commit_sha: "b82e19d0",
        passed: true,
        log_path: "C:/beehaven/logs/test-task-4.log",
        output_preview: "running 14 tests\ntest docs::links_valid ... ok\ntest result: ok. 14 passed; 0 failed; finished in 0.31s\n",
        started_at: "2026-11-27T12:21:00Z",
        duration_ms: 310,
      },
    ],
  },
  terminalReplay: {
    "task-3":
      "\x1b[36m[Claude Code]\x1b[0m Inspecting src/auth/login.rs and src/routes/mod.rs...\r\n" +
      "\x1b[33m▲ Question:\x1b[0m Should rate-limited login attempts return HTTP 429 with Retry-After? (y/n): ",
    "task-1":
      "\x1b[36m[Claude Code]\x1b[0m Implementing chunk boundary state machine in src/parser.rs...\r\n",
    "task-2":
      "\x1b[32m✔ Completed:\x1b[0m Refactored token verification and ran `cargo test` (14 passed).\r\n",
    "task-4":
      "[Beehaven Task] Document error codes and rate limits in docs/API.md\r\nPS C:\\projects\\my-app.worktrees\\task4-docs> ",
  },
  queue: {
    running: false,
    paused: false,
    stop_after_current: false,
    items: [],
    halted_reason: null,
    halted_task_id: null,
  },
  listeners: {},
};

function emitMock(event: string, payload: unknown) {
  for (const fn of mockState.listeners[event] ?? []) {
    fn(payload);
  }
}

export function normalizeError(err: unknown): AppErrorPayload {
  if (err && typeof err === "object" && "message" in err) {
    const obj = err as Partial<AppErrorPayload>;
    return {
      code: obj.code ?? "APP_ERROR",
      message: String(obj.message),
      details: obj.details ?? null,
    };
  }
  return {
    code: "UNKNOWN_ERROR",
    message: String(err),
    details: null,
  };
}

export const api = {
  async repoCheck(path: string): Promise<RepoCheckResult> {
    if (isTauriRuntime()) return invoke("repo_check", { path });
    return {
      path,
      is_git_repo: true,
      git_version: "git version 2.50.1",
      git_version_ok: true,
      detected_base_branch: "main",
      available_branches: ["main", "develop"],
      suggested_test_command: "cargo test",
      detected_agents: await this.agentsList(),
      clean_working_tree: true,
    };
  },

  async repoOpen(
    path: string,
    baseBranch?: string,
    testCommand?: string
  ): Promise<Repo> {
    if (isTauriRuntime()) {
      return invoke("repo_open", {
        path,
        baseBranch: baseBranch ?? null,
        testCommand: testCommand ?? null,
      });
    }
    const existing = mockState.repos.find((r) => r.path === path);
    if (existing) return existing;
    const name = path.split(/[\\/]/).filter(Boolean).pop() ?? "repo";
    const repo: Repo = {
      id: `repo-${Date.now()}`,
      name,
      path,
      base_branch: baseBranch || "main",
      test_command: testCommand || "cargo test",
      worktree_root: `${path}.worktrees`,
      created_at: nowIso(),
    };
    mockState.repos.unshift(repo);
    return repo;
  },

  async repoList(): Promise<Repo[]> {
    if (isTauriRuntime()) return invoke("repo_list");
    return [...mockState.repos];
  },

  async repoCreateDemo(): Promise<Repo> {
    if (isTauriRuntime()) return invoke("repo_create_demo");
    return mockState.repos[0]!;
  },

  async worktreeList(repoId: string): Promise<WorktreeInfo[]> {
    if (isTauriRuntime()) return invoke("worktree_list", { repoId });
    return mockState.tasks
      .filter((t) => t.repo_id === repoId && t.state !== "Discarded")
      .map((t) => ({
        path: t.worktree_path,
        head_sha: "a91f4c82",
        branch: t.branch,
        is_bare: false,
        is_detached: false,
        has_uncommitted_changes: t.state === "Running",
      }));
  },

  async agentsList(): Promise<AgentInfo[]> {
    if (isTauriRuntime()) return invoke("agents_list");
    return [
      {
        id: "claude",
        name: "Claude Code",
        installed: true,
        version: "2.1.42 (Claude Code)",
        supports_hooks: true,
      },
      {
        id: "generic",
        name: "Interactive Shell / Custom CLI",
        installed: true,
        version: "PowerShell (Built-in)",
        supports_hooks: false,
      },
    ];
  },

  async taskList(repoId: string): Promise<Task[]> {
    if (isTauriRuntime()) return invoke("task_list", { repoId });
    return mockState.tasks.filter((t) => t.repo_id === repoId);
  },

  async taskCreate(params: {
    repoId: string;
    title: string;
    prompt: string;
    agent?: string;
    autoStart?: boolean;
  }): Promise<Task> {
    if (isTauriRuntime()) {
      return invoke("task_create", {
        repoId: params.repoId,
        title: params.title,
        prompt: params.prompt,
        agent: params.agent ?? null,
        autoStart: params.autoStart ?? true,
      });
    }
    const slug = params.title.toLowerCase().replace(/[^a-z0-9]+/g, "-");
    const activeCount = mockState.tasks.filter(
      (t) => t.state === "Running" || t.state === "WaitingForYou"
    ).length;
    const state: TaskState =
      activeCount < mockState.settings.max_concurrent_agents ? "Running" : "Queued";
    const created: Task = {
      id: `task-${Date.now()}`,
      repo_id: params.repoId,
      title: params.title,
      prompt: params.prompt,
      branch: `beehaven/${slug}`,
      worktree_path: `C:/projects/my-app.worktrees/${slug}`,
      agent: params.agent || mockState.settings.default_agent,
      state,
      backup_patch_path: null,
      last_error: null,
      created_at: nowIso(),
      updated_at: nowIso(),
    };
    mockState.tasks.push(created);
    emitMock("task:state", { task: created, reason: "Task created" });
    return created;
  },

  async taskStart(taskId: string): Promise<Task> {
    if (isTauriRuntime()) return invoke("task_start", { taskId });
    return this.taskSetManualState(taskId, "Running");
  },

  async taskSetManualState(taskId: string, targetState: TaskState): Promise<Task> {
    if (isTauriRuntime()) {
      return invoke("task_set_manual_state", { taskId, targetState });
    }
    const found = mockState.tasks.find((t) => t.id === taskId);
    if (!found) throw new Error("Task not found");
    found.state = targetState;
    found.updated_at = nowIso();
    emitMock("task:state", { task: { ...found }, reason: "Manual state change" });
    return { ...found };
  },

  async taskApprove(taskId: string): Promise<Task> {
    if (isTauriRuntime()) return invoke("task_approve", { taskId });
    return this.taskSetManualState(taskId, "Approved");
  },

  async taskRevise(taskId: string, message: string): Promise<Task> {
    if (isTauriRuntime()) return invoke("task_revise", { taskId, message });
    mockState.terminalReplay[taskId] =
      (mockState.terminalReplay[taskId] ?? "") + `\r\n> ${message}\r\n[Agent] Revising...\r\n`;
    emitMock("session:output", {
      task_id: taskId,
      chunk: `\r\n> ${message}\r\n[Agent] Revising...\r\n`,
    });
    return this.taskSetManualState(taskId, "Running");
  },

  async taskDiscard(taskId: string, force: boolean): Promise<Task> {
    if (isTauriRuntime()) return invoke("task_discard", { taskId, force });
    const found = mockState.tasks.find((t) => t.id === taskId);
    if (!found) throw new Error("Task not found");
    found.state = "Discarded";
    found.backup_patch_path = `C:/beehaven/backups/${taskId}.patch`;
    found.updated_at = nowIso();
    emitMock("task:state", { task: { ...found }, reason: "Discarded with backup patch" });
    return { ...found };
  },

  async taskCommitAll(taskId: string, message: string): Promise<string> {
    if (isTauriRuntime()) return invoke("task_commit_all", { taskId, message });
    return "f391c0de";
  },

  async backupSaveManual(taskId: string): Promise<string> {
    if (isTauriRuntime()) return invoke("backup_save_manual", { taskId });
    return `C:/beehaven/backups/${taskId}-manual.patch`;
  },

  async sessionReplay(taskId: string): Promise<string> {
    if (isTauriRuntime()) return invoke("session_replay", { taskId });
    return mockState.terminalReplay[taskId] ?? "";
  },

  async sessionWrite(taskId: string, data: string): Promise<void> {
    if (isTauriRuntime()) return invoke("session_write", { taskId, data });
    mockState.terminalReplay[taskId] = (mockState.terminalReplay[taskId] ?? "") + data;
    emitMock("session:output", { task_id: taskId, chunk: data });
  },

  async sessionResize(taskId: string, cols: number, rows: number): Promise<void> {
    if (isTauriRuntime()) return invoke("session_resize", { taskId, cols, rows });
  },

  async sessionKill(taskId: string): Promise<Task> {
    if (isTauriRuntime()) return invoke("session_kill", { taskId });
    return this.taskSetManualState(taskId, "NeedsReview");
  },

  async diffSummaryGet(taskId: string): Promise<DiffSummary> {
    if (isTauriRuntime()) return invoke("diff_summary_get", { taskId });
    return {
      task_id: taskId,
      base_branch: "main",
      head_sha: "a91f4c82",
      total_additions: 24,
      total_deletions: 5,
      files: [
        { path: "src/auth/verify.rs", status: "A", additions: 18, deletions: 0 },
        { path: "src/routes/mod.rs", status: "M", additions: 6, deletions: 5 },
      ],
    };
  },

  async diffFileGet(taskId: string, filePath: string): Promise<FileDiff> {
    if (isTauriRuntime()) return invoke("diff_file_get", { taskId, filePath });
    return {
      path: filePath,
      status: filePath.includes("verify") ? "A" : "M",
      additions: 6,
      deletions: 2,
      truncated: false,
      raw_patch: `@@ -10,4 +10,8 @@\n pub fn register_routes() {\n-    mount_legacy_auth();\n+    mount_verified_auth();\n+    enforce_rate_limit(60);\n }\n`,
      hunks: [
        {
          header: "@@ -10,4 +10,8 @@ pub fn register_routes()",
          old_start: 10,
          old_lines: 4,
          new_start: 10,
          new_lines: 8,
          lines: [
            { kind: "context", old_lineno: 10, new_lineno: 10, content: "pub fn register_routes() {" },
            { kind: "del", old_lineno: 11, new_lineno: null, content: "    mount_legacy_auth();" },
            { kind: "add", old_lineno: null, new_lineno: 11, content: "    mount_verified_auth();" },
            { kind: "add", old_lineno: null, new_lineno: 12, content: "    enforce_rate_limit(60);" },
            { kind: "context", old_lineno: 12, new_lineno: 13, content: "}" },
          ],
        },
      ],
    };
  },

  async testsRun(taskId: string): Promise<TestRun> {
    if (isTauriRuntime()) return invoke("tests_run", { taskId });
    const run: TestRun = {
      id: `tr-${Date.now()}`,
      task_id: taskId,
      commit_sha: "a91f4c82",
      passed: true,
      log_path: `C:/beehaven/logs/test-${taskId}.log`,
      output_preview:
        "running 16 tests\ntest parser::streams_chunks ... ok\ntest auth::verify_token ... ok\ntest result: ok. 16 passed; 0 failed; finished in 0.38s\n",
      started_at: nowIso(),
      duration_ms: 380,
    };
    mockState.testRuns[taskId] = [run, ...(mockState.testRuns[taskId] ?? [])];
    emitMock("tests:done", run);
    return run;
  },

  async testsList(taskId: string): Promise<TestRun[]> {
    if (isTauriRuntime()) return invoke("tests_list", { taskId });
    return mockState.testRuns[taskId] ?? [];
  },

  async conflictsCompute(repoId: string): Promise<ConflictMatrixReport> {
    if (isTauriRuntime()) return invoke("conflicts_compute", { repoId });
    const active = mockState.tasks.filter(
      (t) => t.repo_id === repoId && t.state !== "Merged" && t.state !== "Discarded"
    );
    const cells = [];
    for (let i = 0; i < active.length; i++) {
      for (let j = i + 1; j < active.length; j++) {
        const a = active[i]!;
        const b = active[j]!;
        const isConflict =
          (a.id === "task-2" && b.id === "task-3") ||
          (a.id === "task-3" && b.id === "task-2");
        cells.push({
          task_a_id: a.id,
          task_b_id: b.id,
          sha_a: "a91f4c",
          sha_b: "d04b91",
          status: (isConflict ? "Conflicted" : "Clean") as "Conflicted" | "Clean",
          files: isConflict ? ["src/auth/verify.rs", "src/routes/mod.rs"] : [],
          error_message: null,
          computed_at: nowIso(),
        });
      }
    }
    return {
      repo_id: repoId,
      cells,
      suggestion: {
        ordered_task_ids: ["task-4", "task-1", "task-2", "task-3"].filter((id) =>
          active.some((t) => t.id === id)
        ),
        reasoning: [
          "'Update API docs' has 0 conflicts (11 changed lines) — merge cleanly first.",
          "'Parser streaming support' has 0 conflicts (34 changed lines) — merge cleanly second.",
          "'Refactor auth middleware' conflicts with 1 later branch (24 changed lines) — merge before conflicting peers.",
          "'Add login validation' will need a conflict resolution rebase after Refactor auth middleware.",
        ],
      },
    };
  },

  async queueStatus(): Promise<QueueState> {
    if (isTauriRuntime()) return invoke("queue_status");
    return { ...mockState.queue };
  },

  async queueStart(repoId: string, taskIds: string[]): Promise<QueueState> {
    if (isTauriRuntime()) return invoke("queue_start", { repoId, taskIds });
    const items = taskIds.map((id) => {
      const t = mockState.tasks.find((x) => x.id === id);
      if (t) t.state = "Merged";
      return {
        task_id: id,
        title: t?.title ?? id,
        branch: t?.branch ?? id,
        status: "Merged" as const,
        detail: "Merged, tests passed",
      };
    });
    mockState.queue = {
      running: false,
      paused: false,
      stop_after_current: false,
      items,
      halted_reason: null,
      halted_task_id: null,
    };
    emitMock("queue:progress", mockState.queue);
    return { ...mockState.queue };
  },

  async queuePause(paused: boolean): Promise<QueueState> {
    if (isTauriRuntime()) return invoke("queue_pause", { paused });
    mockState.queue.paused = paused;
    return { ...mockState.queue };
  },

  async queueStop(): Promise<QueueState> {
    if (isTauriRuntime()) return invoke("queue_stop");
    mockState.queue.stop_after_current = true;
    return { ...mockState.queue };
  },

  async settingsGet(): Promise<AppSettings> {
    if (isTauriRuntime()) return invoke("settings_get");
    return { ...mockState.settings };
  },

  async settingsSave(settings: AppSettings): Promise<AppSettings> {
    if (isTauriRuntime()) return invoke("settings_save", { settings });
    mockState.settings = { ...settings };
    return { ...mockState.settings };
  },

  async eventsList(limit = 100): Promise<TaskEvent[]> {
    if (isTauriRuntime()) return invoke("events_list", { limit });
    return [
      {
        id: 1,
        task_id: "task-3",
        at: "2026-11-27T12:20:00Z",
        kind: "task:state",
        detail_json: '{"from":"Running","to":"WaitingForYou","reason":"Agent hook Notification received"}',
      },
    ];
  },

  async diagnosticsExport(destinationPath?: string): Promise<string> {
    if (isTauriRuntime()) {
      return invoke("diagnostics_export", { destinationPath: destinationPath ?? null });
    }
    return "C:/beehaven/beehaven-diagnostics-20261127.json";
  },

  async subscribe<T>(
    event: string,
    handler: (payload: T) => void
  ): Promise<UnlistenFn> {
    if (isTauriRuntime()) {
      return listen<T>(event, (ev) => handler(ev.payload));
    }
    const list = mockState.listeners[event] ?? [];
    const wrapped = (p: unknown) => handler(p as T);
    list.push(wrapped);
    mockState.listeners[event] = list;
    return () => {
      mockState.listeners[event] = (mockState.listeners[event] ?? []).filter(
        (fn) => fn !== wrapped
      );
    };
  },
};
