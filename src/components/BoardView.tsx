import { useAppStore } from "../store/useAppStore";
import type { Task } from "../types/models";
import { StatusBadge } from "./StatusBadge";
import { TaskDetail } from "./TaskDetail";

interface TaskGroup {
  id: string;
  title: string;
  items: Task[];
}

export function BoardView() {
  const repos = useAppStore((s) => s.repos);
  const activeRepoId = useAppStore((s) => s.activeRepoId);
  const tasks = useAppStore((s) => s.tasks);
  const selectedTaskId = useAppStore((s) => s.selectedTaskId);
  const selectTask = useAppStore((s) => s.selectTask);
  const setAddTaskOpen = useAppStore((s) => s.setAddTaskOpen);
  const setOnboardingOpen = useAppStore((s) => s.setOnboardingOpen);
  const createDemoRepo = useAppStore((s) => s.createDemoRepo);
  const showToast = useAppStore((s) => s.showToast);

  const activeRepo = repos.find((r) => r.id === activeRepoId) ?? null;

  if (!activeRepo) {
    return (
      <div className="mx-auto flex h-full max-w-3xl flex-col items-center justify-center gap-6 overflow-y-auto p-8 text-center">
        <div>
          <h2 className="m-0 text-2xl font-bold">
            Welcome to Beehaven — Your Parallel Agent Hive
          </h2>
          <p className="mx-auto mt-2 max-w-xl text-sm" style={{ color: "var(--af-muted)" }}>
            Send multiple AI coding agents to work at once without them stepping on each other’s files. Come back to tested, conflict-checked branches ready to merge.
          </p>
        </div>

        <div className="grid w-full grid-cols-1 gap-3 text-left sm:grid-cols-2">
          <div
            style={{
              backgroundColor: "var(--af-surface)",
              borderColor: "var(--af-border)",
              borderRadius: "var(--af-radius-panel)",
            }}
            className="border p-3.5 text-xs"
          >
            <strong className="text-sm">1. Isolated Honeycomb Cells</strong>
            <p className="m-0 mt-1" style={{ color: "var(--af-muted)" }}>
              Every task gets its own Git worktree copy of your folder so parallel agents never overwrite one another.
            </p>
          </div>
          <div
            style={{
              backgroundColor: "var(--af-surface)",
              borderColor: "var(--af-border)",
              borderRadius: "var(--af-radius-panel)",
            }}
            className="border p-3.5 text-xs"
          >
            <strong className="text-sm" style={{ color: "var(--af-attention)" }}>
              2. ▲ Waiting-for-You Priority
            </strong>
            <p className="m-0 mt-1" style={{ color: "var(--af-muted)" }}>
              Calm by default. Only agents asking a question jump to the top so you know where you’re needed in two seconds.
            </p>
          </div>
          <div
            style={{
              backgroundColor: "var(--af-surface)",
              borderColor: "var(--af-border)",
              borderRadius: "var(--af-radius-panel)",
            }}
            className="border p-3.5 text-xs"
          >
            <strong className="text-sm" style={{ color: "var(--af-info)" }}>
              3. Pairwise Conflict Forecast
            </strong>
            <p className="m-0 mt-1" style={{ color: "var(--af-muted)" }}>
              Simulates merges across every pair of active branches ahead of time and suggests the cleanest merge order.
            </p>
          </div>
          <div
            style={{
              backgroundColor: "var(--af-surface)",
              borderColor: "var(--af-border)",
              borderRadius: "var(--af-radius-panel)",
            }}
            className="border p-3.5 text-xs"
          >
            <strong className="text-sm" style={{ color: "var(--af-success)" }}>
              4. Test-Gated Merge Queue
            </strong>
            <p className="m-0 mt-1" style={{ color: "var(--af-muted)" }}>
              Rebases each approved branch, runs your test suite in a clean folder, and fast-forwards only when green.
            </p>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-center gap-3">
          <button
            type="button"
            className="af-btn af-btn-primary px-4 py-2 text-sm"
            onClick={() => setOnboardingOpen(true)}
          >
            Open My Git Repository…
          </button>
          <button
            type="button"
            className="af-btn af-btn-success px-4 py-2 text-sm"
            onClick={() =>
              void createDemoRepo().then((r) => {
                if (r) {
                  showToast(
                    "Created real Honeycomb practice sandbox with 4 branches & conflict demo!"
                  );
                }
              })
            }
          >
            ✓ Try 1-Click Practice Sandbox
          </button>
        </div>
      </div>
    );
  }

  // Exact priority grouping from Section 3.2:
  // 1. Waiting for you first, 2. Running & Queued, 3. To review & Approved, 4. Completed / Failed
  const groups: TaskGroup[] = [
    {
      id: "waiting",
      title: "▲ Waiting for you",
      items: tasks.filter((t) => t.state === "WaitingForYou"),
    },
    {
      id: "active",
      title: "◐ Running & Queued",
      items: tasks.filter(
        (t) => t.state === "Running" || t.state === "Queued" || t.state === "Merging"
      ),
    },
    {
      id: "review",
      title: "◆ To review & Approved",
      items: tasks.filter(
        (t) => t.state === "NeedsReview" || t.state === "Approved"
      ),
    },
    {
      id: "history",
      title: "History (Failed / Merged / Discarded)",
      items: tasks.filter(
        (t) =>
          t.state === "Failed" ||
          t.state === "Merged" ||
          t.state === "Discarded"
      ),
    },
  ].filter((g) => g.items.length > 0);

  const selectedTask =
    tasks.find((t) => t.id === selectedTaskId) ?? tasks[0] ?? null;

  return (
    <div className="grid h-full min-h-0 grid-cols-1 md:grid-cols-[310px_1fr]">
      {/* Left Priority Task List */}
      <aside
        aria-label="Tasks grouped by priority"
        style={{
          backgroundColor: "var(--af-surface)",
          borderRightColor: "var(--af-border)",
        }}
        className="flex min-h-0 flex-col border-r"
      >
        <div
          style={{ borderBottomColor: "var(--af-border)" }}
          className="flex items-center justify-between border-b px-3 py-2.5 text-xs font-semibold uppercase tracking-wide"
        >
          <span style={{ color: "var(--af-muted)" }}>
            Tasks ({tasks.length})
          </span>
          <button
            type="button"
            className="af-btn af-btn-primary py-1 text-xs"
            onClick={() => setAddTaskOpen(true)}
          >
            + Add task
          </button>
        </div>

        {tasks.length === 0 ? (
          /* Exact Empty State Microcopy from Section 3.7 */
          <div className="flex flex-1 flex-col items-start justify-center gap-3 p-5">
            <p className="m-0 text-sm" style={{ color: "var(--af-muted)" }}>
              No tasks yet. Add one and an agent will start in its own copy of your repo.
            </p>
            <button
              type="button"
              className="af-btn af-btn-primary"
              onClick={() => setAddTaskOpen(true)}
            >
              + Add first task
            </button>
          </div>
        ) : (
          <div className="flex-1 overflow-y-auto p-2">
            {groups.map((group) => (
              <div key={group.id} className="mb-3">
                <div
                  className="px-2 py-1 text-[11px] font-semibold uppercase tracking-wider"
                  style={{
                    color:
                      group.id === "waiting"
                        ? "var(--af-attention)"
                        : "var(--af-muted)",
                  }}
                >
                  {group.title} ({group.items.length})
                </div>
                <ul role="list" className="m-0 flex list-none flex-col gap-1 p-0">
                  {group.items.map((t) => {
                    const isSelected = selectedTask?.id === t.id;
                    return (
                      <li key={t.id}>
                        <button
                          type="button"
                          onClick={() => selectTask(t.id)}
                          style={{
                            backgroundColor: isSelected
                              ? "var(--af-surface-elevated)"
                              : "transparent",
                            borderColor: isSelected
                              ? "var(--af-accent)"
                              : t.state === "WaitingForYou"
                              ? "var(--af-attention)"
                              : "transparent",
                            borderRadius: "var(--af-radius-control)",
                          }}
                          className="flex w-full flex-col gap-1 border px-2.5 py-2 text-left cursor-pointer"
                        >
                          <div className="flex w-full items-center justify-between gap-2">
                            <span className="truncate text-sm font-medium">
                              {t.title}
                            </span>
                            <StatusBadge state={t.state} compact />
                          </div>
                          <div
                            className="af-mono truncate text-[11px]"
                            style={{ color: "var(--af-muted)" }}
                          >
                            {t.branch} · {t.agent}
                          </div>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ))}
          </div>
        )}
      </aside>

      {/* Right Task Detail Inspector */}
      <div className="min-h-0 overflow-hidden">
        {selectedTask ? (
          <TaskDetail
            task={selectedTask}
            testCommand={activeRepo.test_command}
          />
        ) : (
          <div
            className="flex h-full items-center justify-center p-8 text-sm"
            style={{ color: "var(--af-muted)" }}
          >
            Select a task on the left or add a new task to begin.
          </div>
        )}
      </div>
    </div>
  );
}
