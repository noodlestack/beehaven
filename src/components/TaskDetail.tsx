import { useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore, type DetailTab } from "../store/useAppStore";
import type { Task, TaskState } from "../types/models";
import { DiffViewer } from "./DiffViewer";
import { StatusBadge } from "./StatusBadge";
import { TerminalPane } from "./TerminalPane";
import { TestsPane } from "./TestsPane";

interface TaskDetailProps {
  task: Task;
  testCommand: string;
}

export function TaskDetail({ task, testCommand }: TaskDetailProps) {
  const activeTab = useAppStore((s) => s.activeDetailTab);
  const setActiveTab = useAppStore((s) => s.setActiveDetailTab);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const conflictReport = useAppStore((s) => s.conflictReport);
  const tasks = useAppStore((s) => s.tasks);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const refreshConflicts = useAppStore((s) => s.refreshConflicts);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const [reviseOpen, setReviseOpen] = useState(false);
  const [revisePrompt, setRevisePrompt] = useState("");
  const [discardConfirmOpen, setDiscardConfirmOpen] = useState(false);
  const [commitOpen, setCommitOpen] = useState(false);
  const [commitMessage, setCommitMessage] = useState(task.title);

  const taskConflicts = (conflictReport?.cells ?? []).filter(
    (c) =>
      (c.task_a_id === task.id || c.task_b_id === task.id) &&
      c.status !== "Clean"
  );

  const handleApprove = async () => {
    try {
      const updated = await api.taskApprove(task.id);
      upsertTaskLocal(updated);
      showToast(`Approved "${task.title}" for the merge queue.`);
    } catch (err) {
      setError(err);
    }
  };

  const handleSendRevision = async () => {
    if (!revisePrompt.trim()) return;
    try {
      const updated = await api.taskRevise(task.id, revisePrompt.trim());
      upsertTaskLocal(updated);
      setRevisePrompt("");
      setReviseOpen(false);
      setActiveTab("terminal");
      showToast("Sent revision instructions to the agent.");
    } catch (err) {
      setError(err);
    }
  };

  const handleConfirmDiscard = async () => {
    try {
      const updated = await api.taskDiscard(task.id, true);
      upsertTaskLocal(updated);
      setDiscardConfirmOpen(false);
      showToast(
        updated.backup_patch_path
          ? `Discarded task. Backup patch saved at ${updated.backup_patch_path}`
          : "Discarded task safely."
      );
    } catch (err) {
      setError(err);
    }
  };

  const handleCommitWorktree = async () => {
    try {
      const sha = await api.taskCommitAll(task.id, commitMessage);
      setCommitOpen(false);
      showToast(`Committed worktree changes (${sha.slice(0, 8)}).`);
      await refreshConflicts();
    } catch (err) {
      setError(err);
    }
  };

  const handleManualState = async (target: TaskState) => {
    try {
      const updated = await api.taskSetManualState(task.id, target);
      upsertTaskLocal(updated);
    } catch (err) {
      setError(err);
    }
  };

  const tabs: Array<{ id: DetailTab; label: string }> = [
    { id: "terminal", label: "Terminal" },
    { id: "changes", label: "Changes" },
    { id: "tests", label: "Tests" },
    {
      id: "conflicts",
      label:
        taskConflicts.length > 0
          ? `Conflicts (${taskConflicts.length})`
          : "Conflicts",
    },
  ];

  return (
    <section
      aria-label={`Details for task ${task.title}`}
      className="flex h-full flex-col gap-3 p-4"
    >
      {/* Detail Header */}
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex flex-wrap items-center gap-2.5">
            <h2 className="m-0 text-lg font-semibold">{task.title}</h2>
            <StatusBadge state={task.state} />
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-xs" style={{ color: "var(--af-muted)" }}>
            <span>
              branch: <code className="af-mono">{task.branch}</code>
            </span>
            <span>·</span>
            <span>Prompt: {task.prompt}</span>
          </div>
          {task.last_error && (
            <div
              role="alert"
              style={{
                borderColor: "var(--af-danger)",
                color: "var(--af-danger)",
                backgroundColor: "var(--af-surface-elevated)",
                borderRadius: "var(--af-radius-control)",
              }}
              className="mt-2 border px-3 py-2 text-xs"
            >
              ✗ {task.last_error}
            </div>
          )}
          {task.backup_patch_path && (
            <div className="af-mono mt-1 text-xs" style={{ color: "var(--af-muted)" }}>
              Backup patch saved: {task.backup_patch_path}
            </div>
          )}
        </div>

        {/* Manual State Toggle (Fallback for Generic CLI agents — Section 4.5) */}
        {task.state !== "Merged" && task.state !== "Discarded" && (
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <label htmlFor="manual-state-select" className="whitespace-nowrap" style={{ color: "var(--af-muted)" }}>
              Set status:
            </label>
            <select
              id="manual-state-select"
              value={task.state}
              onChange={(e) => void handleManualState(e.target.value as TaskState)}
              className="af-input w-auto py-1"
            >
              <option value="Queued">○ Queued</option>
              <option value="Running">◐ Running</option>
              <option value="WaitingForYou">▲ Waiting for you</option>
              <option value="NeedsReview">◆ Needs review</option>
              <option value="Approved">✓ Approved</option>
              <option value="Merging">⟳ Merging</option>
              <option value="Failed">✗ Failed</option>
            </select>
            <button
              type="button"
              className="af-btn whitespace-nowrap"
              onClick={() => {
                setCommitMessage(task.title);
                setCommitOpen((v) => !v);
              }}
            >
              Commit edits…
            </button>
          </div>
        )}
      </div>

      {commitOpen && (
        <div
          style={{
            backgroundColor: "var(--af-surface)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="flex items-center gap-2 border p-2.5"
        >
          <input
            type="text"
            aria-label="Commit message"
            className="af-input"
            value={commitMessage}
            onChange={(e) => setCommitMessage(e.target.value)}
            placeholder="Commit message for all worktree changes"
          />
          <button
            type="button"
            className="af-btn af-btn-primary shrink-0"
            onClick={() => void handleCommitWorktree()}
          >
            Stage & Commit
          </button>
          <button
            type="button"
            className="af-btn shrink-0"
            onClick={() => setCommitOpen(false)}
          >
            Cancel
          </button>
        </div>
      )}

      {/* Sub-navigation Tabs */}
      <div
        role="tablist"
        aria-label="Task inspection tabs"
        style={{ borderBottomColor: "var(--af-border)" }}
        className="flex flex-wrap gap-1 border-b pb-2"
      >
        {tabs.map((t) => {
          const selected = activeTab === t.id;
          return (
            <button
              key={t.id}
              id={`tab-${t.id}`}
              role="tab"
              type="button"
              aria-selected={selected}
              aria-controls={`tabpanel-${t.id}`}
              onClick={() => setActiveTab(t.id)}
              className={`af-btn ${selected ? "af-btn-primary" : ""}`}
            >
              {t.label}
            </button>
          );
        })}
      </div>

      {/* Tab Body */}
      <div
        id={`tabpanel-${activeTab}`}
        role="tabpanel"
        aria-labelledby={`tab-${activeTab}`}
        className="min-h-0 flex-1 overflow-auto"
      >
        {activeTab === "terminal" && <TerminalPane task={task} />}
        {activeTab === "changes" && <DiffViewer task={task} />}
        {activeTab === "tests" && (
          <TestsPane task={task} testCommand={testCommand} />
        )}
        {activeTab === "conflicts" && (
          <div className="flex flex-col gap-3 p-2">
            <div className="flex items-center justify-between">
              <p className="m-0 text-sm">
                {taskConflicts.length === 0
                  ? "✓ Clean against all other active task branches."
                  : `✗ Conflicts forecast with ${taskConflicts.length} peer task branch(es).`}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  className="af-btn"
                  onClick={() => void refreshConflicts()}
                >
                  ↻ Recompute
                </button>
                <button
                  type="button"
                  className="af-btn af-btn-primary"
                  onClick={() => setActiveScreen("conflicts")}
                >
                  Open full Conflict Matrix →
                </button>
              </div>
            </div>

            {taskConflicts.map((cell) => {
              const peerId =
                cell.task_a_id === task.id ? cell.task_b_id : cell.task_a_id;
              const peer = tasks.find((x) => x.id === peerId);
              return (
                <div
                  key={`${cell.task_a_id}-${cell.task_b_id}`}
                  style={{
                    backgroundColor: "var(--af-surface)",
                    borderColor: "var(--af-danger)",
                    borderRadius: "var(--af-radius-panel)",
                  }}
                  className="border p-3 text-xs"
                >
                  <div className="font-semibold" style={{ color: "var(--af-danger)" }}>
                    ✗ {cell.files.length} conflicting file(s) with “{peer?.title ?? peerId}”
                  </div>
                  <ul className="af-mono mt-1.5 mb-2 pl-5">
                    {cell.files.map((f) => (
                      <li key={f}>{f}</li>
                    ))}
                  </ul>
                  <button
                    type="button"
                    className="af-btn"
                    onClick={() => {
                      setRevisePrompt(
                        `Coordinate edits on ${cell.files.join(", ")} to avoid conflicting with branch ${peer?.branch ?? peerId}.`
                      );
                      setReviseOpen(true);
                    }}
                  >
                    Ask agent to resolve…
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Inline Ask Agent to Revise bar */}
      {reviseOpen && (
        <div
          style={{
            backgroundColor: "var(--af-surface)",
            borderColor: "var(--af-accent)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="flex items-center gap-2 border p-3"
        >
          <input
            type="text"
            aria-label="Revision instructions for agent"
            className="af-input"
            placeholder="Describe what the agent should revise or resolve…"
            value={revisePrompt}
            onChange={(e) => setRevisePrompt(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void handleSendRevision();
            }}
          />
          <button
            type="button"
            className="af-btn af-btn-primary shrink-0"
            onClick={() => void handleSendRevision()}
          >
            Send to agent
          </button>
          <button
            type="button"
            className="af-btn shrink-0"
            onClick={() => setReviseOpen(false)}
          >
            Cancel
          </button>
        </div>
      )}

      {/* Destructive Discard Confirmation (Section 3.7 Microcopy) */}
      {discardConfirmOpen && (
        <div
          role="alertdialog"
          aria-label="Confirm task discard"
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderColor: "var(--af-danger)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="flex flex-wrap items-center justify-between gap-3 border p-3 text-xs"
        >
          <span>
            Discard “{task.title}”? Its branch (
            <code className="af-mono">{task.branch}</code>) and worktree files will be
            deleted. A backup patch is saved first.
          </span>
          <div className="flex gap-2">
            <button
              type="button"
              className="af-btn af-btn-danger"
              onClick={() => void handleConfirmDiscard()}
            >
              ⊘ Discard (save backup patch)
            </button>
            <button
              type="button"
              className="af-btn"
              onClick={() => setDiscardConfirmOpen(false)}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Primary Review Actions Footer (Section 3.2) */}
      {task.state !== "Merged" && task.state !== "Discarded" && (
        <div
          style={{ borderTopColor: "var(--af-border)" }}
          className="flex flex-wrap items-center justify-between gap-2 border-t pt-3"
        >
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              className="af-btn af-btn-success"
              disabled={task.state === "Approved" || task.state === "Merging"}
              onClick={() => void handleApprove()}
            >
              ✓ Approve
            </button>
            <button
              type="button"
              className="af-btn"
              onClick={() => setReviseOpen((v) => !v)}
            >
              Ask agent to revise
            </button>
            <button
              type="button"
              className="af-btn"
              onClick={() =>
                void api
                  .backupSaveManual(task.id)
                  .then((p) => showToast(`Saved backup patch: ${p}`))
                  .catch(setError)
              }
            >
              Save backup patch
            </button>
          </div>
          <button
            type="button"
            className="af-btn af-btn-danger"
            onClick={() => setDiscardConfirmOpen(true)}
          >
            ⊘ Discard
          </button>
        </div>
      )}
    </section>
  );
}
