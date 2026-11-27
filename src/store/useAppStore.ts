import { create } from "zustand";
import { api, normalizeError } from "../lib/ipc";
import { applyThemeTokens, resolveEffectiveTheme } from "../lib/tokens";
import type {
  AgentInfo,
  AppErrorPayload,
  AppSettings,
  ConflictMatrixReport,
  QueueState,
  Repo,
  Task,
  TestRun,
  WorktreeInfo,
} from "../types/models";

export type ActiveScreen = "board" | "conflicts" | "queue" | "settings";
export type DetailTab = "terminal" | "changes" | "tests" | "conflicts";

interface AppStoreState {
  initialized: boolean;
  repos: Repo[];
  activeRepoId: string | null;
  tasks: Task[];
  selectedTaskId: string | null;
  activeScreen: ActiveScreen;
  activeDetailTab: DetailTab;
  agents: AgentInfo[];
  worktrees: WorktreeInfo[];
  conflictReport: ConflictMatrixReport | null;
  queue: QueueState;
  settings: AppSettings;
  latestTestRuns: Record<string, TestRun[]>;
  bannerError: AppErrorPayload | null;
  toastMessage: string | null;
  addTaskOpen: boolean;
  commandPaletteOpen: boolean;
  onboardingOpen: boolean;

  init: () => Promise<void>;
  setActiveScreen: (screen: ActiveScreen) => void;
  setActiveDetailTab: (tab: DetailTab) => void;
  selectTask: (taskId: string | null) => void;
  setAddTaskOpen: (open: boolean) => void;
  setCommandPaletteOpen: (open: boolean) => void;
  setOnboardingOpen: (open: boolean) => void;
  clearError: () => void;
  showToast: (msg: string) => void;
  setError: (err: unknown) => void;

  selectRepo: (repoId: string) => Promise<void>;
  openRepo: (path: string, baseBranch?: string, testCommand?: string) => Promise<Repo | null>;
  createDemoRepo: () => Promise<Repo | null>;
  refreshTasks: () => Promise<void>;
  refreshConflicts: () => Promise<void>;
  refreshWorktrees: () => Promise<void>;
  updateSettings: (next: AppSettings) => Promise<void>;
  upsertTaskLocal: (task: Task) => void;
  recordTestRunLocal: (run: TestRun) => void;
  setQueueStateLocal: (queue: QueueState) => void;
}

export const useAppStore = create<AppStoreState>((set, get) => ({
  initialized: false,
  repos: [],
  activeRepoId: null,
  tasks: [],
  selectedTaskId: null,
  activeScreen: "board",
  activeDetailTab: "terminal",
  agents: [],
  worktrees: [],
  conflictReport: null,
  queue: {
    running: false,
    paused: false,
    stop_after_current: false,
    items: [],
    halted_reason: null,
    halted_task_id: null,
  },
  settings: {
    max_concurrent_agents: 4,
    silence_timeout_secs: 20,
    theme: "system",
    screen_reader_mode: false,
    onboarding_completed: false,
    default_agent: "claude",
  },
  latestTestRuns: {},
  bannerError: null,
  toastMessage: null,
  addTaskOpen: false,
  commandPaletteOpen: false,
  onboardingOpen: false,

  init: async () => {
    try {
      const [settings, repos, agents, queue] = await Promise.all([
        api.settingsGet(),
        api.repoList(),
        api.agentsList(),
        api.queueStatus(),
      ]);
      applyThemeTokens(resolveEffectiveTheme(settings.theme));
      const firstRepo = repos[0] ?? null;
      let tasks: Task[] = [];
      let worktrees: WorktreeInfo[] = [];
      let conflictReport: ConflictMatrixReport | null = null;
      const latestTestRuns: Record<string, TestRun[]> = {};

      if (firstRepo) {
        [tasks, worktrees] = await Promise.all([
          api.taskList(firstRepo.id),
          api.worktreeList(firstRepo.id).catch(() => []),
        ]);
        for (const t of tasks) {
          const runs = await api.testsList(t.id).catch(() => []);
          if (runs.length > 0) latestTestRuns[t.id] = runs;
        }
        if (tasks.length >= 2) {
          conflictReport = await api.conflictsCompute(firstRepo.id).catch(() => null);
        }
      }

      // Prioritize selecting a WaitingForYou task if one exists!
      const waiting = tasks.find((t) => t.state === "WaitingForYou");
      const defaultSelected = waiting?.id ?? tasks[0]?.id ?? null;

      set({
        initialized: true,
        settings,
        repos,
        activeRepoId: firstRepo?.id ?? null,
        tasks,
        selectedTaskId: defaultSelected,
        agents,
        worktrees,
        conflictReport,
        queue,
        latestTestRuns,
        onboardingOpen: !settings.onboarding_completed && repos.length === 0,
      });
    } catch (err) {
      set({ initialized: true, bannerError: normalizeError(err) });
    }
  },

  setActiveScreen: (activeScreen) => set({ activeScreen }),
  setActiveDetailTab: (activeDetailTab) => set({ activeDetailTab }),
  selectTask: (selectedTaskId) => set({ selectedTaskId }),
  setAddTaskOpen: (addTaskOpen) => set({ addTaskOpen }),
  setCommandPaletteOpen: (commandPaletteOpen) => set({ commandPaletteOpen }),
  setOnboardingOpen: (onboardingOpen) => set({ onboardingOpen }),
  clearError: () => set({ bannerError: null }),
  showToast: (msg) => {
    set({ toastMessage: msg });
    setTimeout(() => {
      if (get().toastMessage === msg) set({ toastMessage: null });
    }, 3500);
  },
  setError: (err) => set({ bannerError: normalizeError(err) }),

  selectRepo: async (repoId) => {
    try {
      const [tasks, worktrees] = await Promise.all([
        api.taskList(repoId),
        api.worktreeList(repoId).catch(() => []),
      ]);
      const conflictReport =
        tasks.length >= 2 ? await api.conflictsCompute(repoId).catch(() => null) : null;
      const waiting = tasks.find((t) => t.state === "WaitingForYou");
      set({
        activeRepoId: repoId,
        tasks,
        worktrees,
        conflictReport,
        selectedTaskId: waiting?.id ?? tasks[0]?.id ?? null,
      });
    } catch (err) {
      set({ bannerError: normalizeError(err) });
    }
  },

  openRepo: async (path, baseBranch, testCommand) => {
    try {
      const repo = await api.repoOpen(path, baseBranch, testCommand);
      const repos = await api.repoList();
      set({ repos });
      await get().selectRepo(repo.id);
      return repo;
    } catch (err) {
      set({ bannerError: normalizeError(err) });
      return null;
    }
  },

  createDemoRepo: async () => {
    try {
      const repo = await api.repoCreateDemo();
      const repos = await api.repoList();
      set({ repos, onboardingOpen: false });
      await get().selectRepo(repo.id);
      return repo;
    } catch (err) {
      set({ bannerError: normalizeError(err) });
      return null;
    }
  },

  refreshTasks: async () => {
    const repoId = get().activeRepoId;
    if (!repoId) return;
    const tasks = await api.taskList(repoId);
    const currSel = get().selectedTaskId;
    const stillExists = tasks.some((t) => t.id === currSel);
    set({
      tasks,
      selectedTaskId: stillExists ? currSel : (tasks[0]?.id ?? null),
    });
  },

  refreshConflicts: async () => {
    const repoId = get().activeRepoId;
    if (!repoId) return;
    try {
      const conflictReport = await api.conflictsCompute(repoId);
      set({ conflictReport });
    } catch (err) {
      set({ bannerError: normalizeError(err) });
    }
  },

  refreshWorktrees: async () => {
    const repoId = get().activeRepoId;
    if (!repoId) return;
    const worktrees = await api.worktreeList(repoId).catch(() => []);
    set({ worktrees });
  },

  updateSettings: async (next) => {
    try {
      const saved = await api.settingsSave(next);
      applyThemeTokens(resolveEffectiveTheme(saved.theme));
      set({ settings: saved });
    } catch (err) {
      set({ bannerError: normalizeError(err) });
    }
  },

  upsertTaskLocal: (updated) => {
    const list = get().tasks;
    const exists = list.some((t) => t.id === updated.id);
    const nextTasks = exists
      ? list.map((t) => (t.id === updated.id ? updated : t))
      : [...list, updated];
    set({
      tasks: nextTasks,
      selectedTaskId: get().selectedTaskId ?? updated.id,
    });
  },

  recordTestRunLocal: (run) => {
    const prev = get().latestTestRuns[run.task_id] ?? [];
    set({
      latestTestRuns: {
        ...get().latestTestRuns,
        [run.task_id]: [run, ...prev.filter((r) => r.id !== run.id)],
      },
    });
  },

  setQueueStateLocal: (queue) => set({ queue }),
}));
