import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { ConflictCell } from "../types/models";
import { StatusBadge } from "./StatusBadge";

export function ConflictMatrixView() {
  const activeRepoId = useAppStore((s) => s.activeRepoId);
  const tasks = useAppStore((s) => s.tasks);
  const report = useAppStore((s) => s.conflictReport);
  const refreshConflicts = useAppStore((s) => s.refreshConflicts);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const selectTask = useAppStore((s) => s.selectTask);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const setActiveDetailTab = useAppStore((s) => s.setActiveDetailTab);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const [selectedCell, setSelectedCell] = useState<ConflictCell | null>(null);
  const [computing, setComputing] = useState(false);

  const activeTasks = tasks.filter(
    (t) => t.state !== "Merged" && t.state !== "Discarded"
  );

  useEffect(() => {
    if (activeRepoId && !report && activeTasks.length >= 2) {
      void refreshConflicts();
    }
  }, [activeRepoId, activeTasks.length]);

  const handleRecompute = async () => {
    setComputing(true);
    try {
      await refreshConflicts();
      showToast("Conflict forecast updated via `git merge-tree --write-tree -z`.");
    } finally {
      setComputing(false);
    }
  };

  const findCell = (idA: string, idB: string): ConflictCell | undefined => {
    return report?.cells.find(
      (c) =>
        (c.task_a_id === idA && c.task_b_id === idB) ||
        (c.task_a_id === idB && c.task_b_id === idA)
    );
  };

  const handleAskAgentResolve = async (targetTaskId: string, peerTitle: string, files: string[]) => {
    try {
      const prompt = `Resolve potential merge conflicts with "${peerTitle}" in: ${files.join(", ")}. Keep changes minimal and run tests.`;
      const updated = await api.taskRevise(targetTaskId, prompt);
      upsertTaskLocal(updated);
      selectTask(targetTaskId);
      setActiveDetailTab("terminal");
      setActiveScreen("board");
      showToast("Sent conflict resolution instructions to the agent terminal.");
    } catch (err) {
      setError(err);
    }
  };

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="m-0 text-xl font-semibold">Pairwise Conflict Forecast</h2>
          <p className="m-0 mt-1 text-xs" style={{ color: "var(--af-muted)" }}>
            Simulates real three-way merges across every active task branch pair with{" "}
            <code className="af-mono">git merge-tree --write-tree -z</code> without touching working files.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            className="af-btn af-btn-primary"
            disabled={computing}
            onClick={() => void handleRecompute()}
          >
            {computing ? "◐ Forecasting…" : "↻ Recompute forecast"}
          </button>
          <button
            type="button"
            className="af-btn"
            onClick={() => setActiveScreen("queue")}
          >
            Go to Merge Queue →
          </button>
        </div>
      </div>

      {activeTasks.length < 2 ? (
        <div
          style={{
            backgroundColor: "var(--af-surface)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="border p-6 text-sm"
        >
          Need at least two active (unmerged, undiscarded) task branches to forecast pairwise conflicts.
        </div>
      ) : (
        <>
          {/* Matrix Table (Section 3.2) */}
          <div
            style={{
              backgroundColor: "var(--af-surface)",
              borderColor: "var(--af-border)",
              borderRadius: "var(--af-radius-panel)",
            }}
            className="overflow-x-auto border p-3"
          >
            <table className="w-full border-collapse text-xs">
              <thead>
                <tr>
                  <th className="p-2.5 text-left" style={{ color: "var(--af-muted)" }}>
                    Task Branch
                  </th>
                  {activeTasks.map((col, idx) => (
                    <th key={col.id} className="p-2.5 text-center font-semibold">
                      {idx + 1}. {col.title}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {activeTasks.map((row, rIdx) => (
                  <tr
                    key={row.id}
                    className="border-t"
                    style={{ borderColor: "var(--af-border)" }}
                  >
                    <th className="p-2.5 text-left font-medium">
                      <div className="flex items-center gap-2">
                        <span>
                          {rIdx + 1}. {row.title}
                        </span>
                        <StatusBadge state={row.state} compact />
                      </div>
                    </th>
                    {activeTasks.map((col) => {
                      if (row.id === col.id) {
                        return (
                          <td
                            key={col.id}
                            className="af-mono p-2.5 text-center"
                            style={{ color: "var(--af-muted)" }}
                          >
                            —
                          </td>
                        );
                      }
                      const cell = findCell(row.id, col.id);
                      if (!cell) {
                        return (
                          <td
                            key={col.id}
                            className="p-2.5 text-center"
                            style={{ color: "var(--af-muted)" }}
                          >
                            ○ pending
                          </td>
                        );
                      }
                      if (cell.status === "Clean") {
                        return (
                          <td
                            key={col.id}
                            className="p-2.5 text-center font-medium"
                            style={{ color: "var(--af-success)" }}
                          >
                            ✓ clean
                          </td>
                        );
                      }
                      if (cell.status === "Error") {
                        return (
                          <td key={col.id} className="p-2.5 text-center">
                            <button
                              type="button"
                              onClick={() => setSelectedCell(cell)}
                              className="af-btn af-btn-danger py-1 text-xs"
                            >
                              ✗ check error
                            </button>
                          </td>
                        );
                      }
                      return (
                        <td key={col.id} className="p-2.5 text-center">
                          <button
                            type="button"
                            onClick={() => setSelectedCell(cell)}
                            className="af-btn af-btn-danger whitespace-nowrap py-1 text-xs"
                          >
                            {`✗ ${cell.files.length} ${cell.files.length === 1 ? "file" : "files"}`}
                          </button>
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Conflict Cell Inspector */}
          {selectedCell && (
            <div
              style={{
                backgroundColor: "var(--af-surface-elevated)",
                borderColor: "var(--af-danger)",
                borderRadius: "var(--af-radius-panel)",
              }}
              className="border p-4 text-xs"
            >
              {(() => {
                const ta = tasks.find((t) => t.id === selectedCell.task_a_id);
                const tb = tasks.find((t) => t.id === selectedCell.task_b_id);
                return (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <strong className="text-sm" style={{ color: "var(--af-danger)" }}>
                        {selectedCell.status === "Error"
                          ? `✗ Merge simulation error between "${ta?.title}" and "${tb?.title}"`
                          : `✗ Conflicting files between "${ta?.title}" and "${tb?.title}"`}
                      </strong>
                      <button
                        type="button"
                        className="af-btn py-1"
                        onClick={() => setSelectedCell(null)}
                      >
                        Close
                      </button>
                    </div>
                    {selectedCell.error_message && (
                      <pre className="af-mono m-0 whitespace-pre-wrap">
                        {selectedCell.error_message}
                      </pre>
                    )}
                    {selectedCell.files.length > 0 && (
                      <ul className="af-mono m-0 pl-5">
                        {selectedCell.files.map((f) => (
                          <li key={f}>{f}</li>
                        ))}
                      </ul>
                    )}
                    <div className="mt-2 flex flex-wrap gap-2">
                      {ta && (
                        <button
                          type="button"
                          className="af-btn"
                          onClick={() =>
                            void handleAskAgentResolve(
                              ta.id,
                              tb?.title ?? "peer branch",
                              selectedCell.files
                            )
                          }
                        >
                          Ask “{ta.title}” agent to resolve
                        </button>
                      )}
                      {tb && (
                        <button
                          type="button"
                          className="af-btn"
                          onClick={() =>
                            void handleAskAgentResolve(
                              tb.id,
                              ta?.title ?? "peer branch",
                              selectedCell.files
                            )
                          }
                        >
                          Ask “{tb.title}” agent to resolve
                        </button>
                      )}
                    </div>
                  </div>
                );
              })()}
            </div>
          )}

          {/* Suggested Merge Order (Section 3.2 & Scope S5) */}
          {report && (
            <div
              style={{
                backgroundColor: "var(--af-surface)",
                borderColor: "var(--af-border)",
                borderRadius: "var(--af-radius-panel)",
              }}
              className="border p-4"
            >
              <h3 className="m-0 text-sm font-semibold">
                Suggested Merge Order (Greedy: fewest conflicts first, smaller diff tie-breaker)
              </h3>
              <div className="mt-2 flex flex-wrap items-center gap-2 text-xs">
                {report.suggestion.ordered_task_ids.map((id, idx) => {
                  const t = tasks.find((x) => x.id === id);
                  return (
                    <span
                      key={id}
                      style={{
                        backgroundColor: "var(--af-surface-elevated)",
                        borderColor: "var(--af-border)",
                        borderRadius: "var(--af-radius-control)",
                      }}
                      className="border px-2.5 py-1 font-medium"
                    >
                      {idx + 1}. {t?.title ?? id}
                    </span>
                  );
                })}
              </div>
              <ul
                className="mt-3 mb-0 flex flex-col gap-1 pl-5 text-xs"
                style={{ color: "var(--af-muted)" }}
              >
                {report.suggestion.reasoning.map((line, i) => (
                  <li key={i}>{line}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </div>
  );
}
