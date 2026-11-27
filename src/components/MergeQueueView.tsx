import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

export function MergeQueueView() {
  const activeRepoId = useAppStore((s) => s.activeRepoId);
  const repos = useAppStore((s) => s.repos);
  const tasks = useAppStore((s) => s.tasks);
  const conflictReport = useAppStore((s) => s.conflictReport);
  const queue = useAppStore((s) => s.queue);
  const worktrees = useAppStore((s) => s.worktrees);
  const setQueueStateLocal = useAppStore((s) => s.setQueueStateLocal);
  const refreshTasks = useAppStore((s) => s.refreshTasks);
  const refreshWorktrees = useAppStore((s) => s.refreshWorktrees);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const selectTask = useAppStore((s) => s.selectTask);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const setActiveDetailTab = useAppStore((s) => s.setActiveDetailTab);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const activeRepo = repos.find((r) => r.id === activeRepoId) ?? null;

  // Order approved tasks according to suggested merge order if available!
  const approvedTasks = tasks
    .filter((t) => t.state === "Approved")
    .sort((a, b) => {
      const order = conflictReport?.suggestion.ordered_task_ids ?? [];
      const ia = order.indexOf(a.id);
      const ib = order.indexOf(b.id);
      if (ia === -1 && ib === -1) return 0;
      if (ia === -1) return 1;
      if (ib === -1) return -1;
      return ia - ib;
    });

  const approvedIdsKey = approvedTasks.map((t) => t.id).join(",");

  const [selectedIds, setSelectedIds] = useState<string[]>(
    approvedTasks.map((t) => t.id)
  );

  useEffect(() => {
    setSelectedIds(approvedTasks.map((t) => t.id));
  }, [approvedIdsKey, conflictReport]);

  const validSelectedIds = selectedIds.filter((id) =>
    approvedTasks.some((t) => t.id === id)
  );

  useEffect(() => {
    void refreshWorktrees();
  }, [activeRepoId]);

  const moveItem = (id: string, dir: -1 | 1) => {
    const idx = selectedIds.indexOf(id);
    if (idx === -1) return;
    const target = idx + dir;
    if (target < 0 || target >= selectedIds.length) return;
    const copy = [...selectedIds];
    const tmp = copy[idx]!;
    copy[idx] = copy[target]!;
    copy[target] = tmp;
    setSelectedIds(copy);
  };

  const toggleId = (id: string) => {
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]
    );
  };

  const handleStartQueue = async () => {
    if (!activeRepoId || validSelectedIds.length === 0) return;
    try {
      const finalState = await api.queueStart(activeRepoId, validSelectedIds);
      setQueueStateLocal(finalState);
      await refreshTasks();
      await refreshWorktrees();
      if (!finalState.halted_reason) {
        showToast("Merge queue completed cleanly with all tests passing.");
      }
    } catch (err) {
      setError(err);
    }
  };

  const handlePauseToggle = async () => {
    try {
      const snap = await api.queuePause(!queue.paused);
      setQueueStateLocal(snap);
    } catch (err) {
      setError(err);
    }
  };

  const handleStopAfterCurrent = async () => {
    try {
      const snap = await api.queueStop();
      setQueueStateLocal(snap);
    } catch (err) {
      setError(err);
    }
  };

  const handleResolveHalted = async () => {
    if (!queue.halted_task_id) return;
    try {
      const reason = queue.halted_reason ?? "Merge queue check failed";
      const updated = await api.taskRevise(
        queue.halted_task_id,
        `Rebase onto ${activeRepo?.base_branch ?? "main"} and fix merge queue failure: ${reason}`
      );
      upsertTaskLocal(updated);
      selectTask(queue.halted_task_id);
      setActiveDetailTab("terminal");
      setActiveScreen("board");
    } catch (err) {
      setError(err);
    }
  };

  const statusMark = (st: string) => {
    switch (st) {
      case "Merged":
        return { mark: "✔", color: "var(--af-success)" };
      case "Rebasing":
      case "Testing":
      case "Merging":
        return { mark: "⟳", color: "var(--af-info)" };
      case "Failed":
        return { mark: "✗", color: "var(--af-danger)" };
      case "Stopped":
        return { mark: "⊘", color: "var(--af-muted)" };
      default:
        return { mark: "○", color: "var(--af-muted)" };
    }
  };

  return (
    <div className="flex h-full flex-col gap-5 overflow-y-auto p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="m-0 text-xl font-semibold">Test-Gated Merge Queue</h2>
          <p className="m-0 mt-1 text-xs" style={{ color: "var(--af-muted)" }}>
            Rebases each approved branch onto{" "}
            <code className="af-mono">{activeRepo?.base_branch ?? "main"}</code>, runs{" "}
            <code className="af-mono">{activeRepo?.test_command ?? "tests"}</code> in an
            isolated verification worktree, and fast-forwards only when green.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="af-btn af-btn-success"
            disabled={queue.running || validSelectedIds.length === 0}
            onClick={() => void handleStartQueue()}
          >
            {`▶ Merge ${validSelectedIds.length} approved ${validSelectedIds.length === 1 ? "task" : "tasks"}`}
          </button>
          <button
            type="button"
            className="af-btn"
            disabled={!queue.running}
            onClick={() => void handlePauseToggle()}
          >
            {queue.paused ? "▶ Resume" : "⏸ Pause"}
          </button>
          <button
            type="button"
            className="af-btn af-btn-danger"
            disabled={!queue.running}
            onClick={() => void handleStopAfterCurrent()}
          >
            Stop after current
          </button>
        </div>
      </div>

      {/* Halted Banner with One Next Action (Section 3.4) */}
      {queue.halted_reason && (
        <div
          role="alert"
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderColor: "var(--af-danger)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="flex flex-wrap items-center justify-between gap-3 border p-4 text-xs"
        >
          <div>
            <div className="text-sm font-semibold" style={{ color: "var(--af-danger)" }}>
              ✗ Queue stopped safely — base branch was left intact
            </div>
            <p className="m-0 mt-1">{queue.halted_reason}</p>
          </div>
          {queue.halted_task_id && (
            <button
              type="button"
              className="af-btn af-btn-primary"
              onClick={() => void handleResolveHalted()}
            >
              Ask agent to resolve →
            </button>
          )}
        </div>
      )}

      {/* Approved Tasks Selection & Ordering */}
      <section
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="border p-4"
      >
        <h3 className="m-0 text-sm font-semibold">
          Approved Tasks Ready to Queue ({approvedTasks.length})
        </h3>
        {approvedTasks.length === 0 ? (
          <p className="m-0 mt-2 text-xs" style={{ color: "var(--af-muted)" }}>
            No tasks are currently marked ✓ Approved. Review a task on the Board and press Approve.
          </p>
        ) : (
          <ul role="list" className="m-0 mt-3 flex list-none flex-col gap-2 p-0">
            {approvedTasks.map((t) => {
              const checked = selectedIds.includes(t.id);
              const orderPos = selectedIds.indexOf(t.id);
              return (
                <li
                  key={t.id}
                  style={{
                    backgroundColor: "var(--af-surface-elevated)",
                    borderColor: "var(--af-border)",
                    borderRadius: "var(--af-radius-control)",
                  }}
                  className="flex items-center justify-between gap-3 border px-3 py-2 text-xs"
                >
                  <label className="flex items-center gap-2.5 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleId(t.id)}
                    />
                    <span className="font-medium">
                      {orderPos >= 0 ? `#${orderPos + 1} ` : ""}
                      {t.title}
                    </span>
                    <code className="af-mono" style={{ color: "var(--af-muted)" }}>
                      ({t.branch})
                    </code>
                  </label>
                  <div className="flex gap-1">
                    <button
                      type="button"
                      className="af-btn px-2 py-0.5"
                      aria-label={`Move ${t.title} earlier`}
                      disabled={!checked || orderPos <= 0}
                      onClick={() => moveItem(t.id, -1)}
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      className="af-btn px-2 py-0.5"
                      aria-label={`Move ${t.title} later`}
                      disabled={!checked || orderPos === selectedIds.length - 1}
                      onClick={() => moveItem(t.id, 1)}
                    >
                      ↓
                    </button>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {/* Live / Recent Queue Execution Log */}
      {queue.items.length > 0 && (
        <section
          style={{
            backgroundColor: "var(--af-surface)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="border p-4"
        >
          <h3 className="m-0 text-sm font-semibold">
            {`Queue Run (${queue.items.length} ${queue.items.length === 1 ? "task" : "tasks"})`}
          </h3>
          <ol role="list" className="m-0 mt-3 flex list-none flex-col gap-2 p-0">
            {queue.items.map((item, i) => {
              const m = statusMark(item.status);
              return (
                <li
                  key={item.task_id}
                  style={{
                    backgroundColor: "var(--af-surface-elevated)",
                    borderColor: "var(--af-border)",
                    borderRadius: "var(--af-radius-control)",
                  }}
                  className="flex flex-wrap items-center justify-between gap-2 border px-3 py-2 text-xs"
                >
                  <span className="font-semibold">
                    {i + 1}. {item.title}{" "}
                    <code className="af-mono" style={{ color: "var(--af-muted)" }}>
                      ({item.branch})
                    </code>
                  </span>
                  <span style={{ color: m.color }} className="font-medium">
                    {m.mark} {item.status} — {item.detail}
                  </span>
                </li>
              );
            })}
          </ol>
        </section>
      )}

      {/* Repository Worktrees Inspector (Scope S1 visibility) */}
      <section
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="border p-4"
      >
        <div className="flex items-center justify-between">
          <h3 className="m-0 text-sm font-semibold">
            Active Git Worktrees ({worktrees.length})
          </h3>
          <button
            type="button"
            className="af-btn py-1 text-xs"
            onClick={() => void refreshWorktrees()}
          >
            ↻ Refresh worktrees
          </button>
        </div>
        <div className="mt-2 flex flex-col gap-1.5 text-xs">
          {worktrees.map((wt) => (
            <div
              key={wt.path}
              className="flex flex-wrap items-center justify-between gap-2 border-t pt-1.5"
              style={{ borderColor: "var(--af-border)" }}
            >
              <span className="af-mono">{wt.path}</span>
              <span style={{ color: "var(--af-muted)" }}>
                {wt.branch ?? "detached"} · {wt.head_sha.slice(0, 8)} ·{" "}
                {wt.has_uncommitted_changes ? "▲ uncommitted edits" : "✓ clean"}
              </span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
