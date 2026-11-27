import { useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

export function AddTaskModal() {
  const open = useAppStore((s) => s.addTaskOpen);
  const setOpen = useAppStore((s) => s.setAddTaskOpen);
  const activeRepoId = useAppStore((s) => s.activeRepoId);
  const agents = useAppStore((s) => s.agents);
  const settings = useAppStore((s) => s.settings);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const selectTask = useAppStore((s) => s.selectTask);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [agent, setAgent] = useState(settings.default_agent);
  const [submitting, setSubmitting] = useState(false);

  if (!open) return null;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeRepoId || !title.trim()) return;
    setSubmitting(true);
    try {
      const created = await api.taskCreate({
        repoId: activeRepoId,
        title: title.trim(),
        prompt: prompt.trim(),
        agent,
        autoStart: true,
      });
      upsertTaskLocal(created);
      selectTask(created.id);
      setActiveScreen("board");
      setTitle("");
      setPrompt("");
      setOpen(false);
      showToast(`Created task "${created.title}" in isolated worktree.`);
    } catch (err) {
      setError(err);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Add new agent task"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onKeyDown={(e) => {
        if (e.key === "Escape") setOpen(false);
      }}
    >
      <form
        onSubmit={(e) => void handleSubmit(e)}
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="flex w-full max-w-lg flex-col gap-4 border p-5 shadow-xl"
      >
        <div className="flex items-center justify-between">
          <h2 className="m-0 text-lg font-semibold">Add Parallel Task</h2>
          <button
            type="button"
            className="af-btn py-1"
            onClick={() => setOpen(false)}
          >
            Esc
          </button>
        </div>

        <label className="flex flex-col gap-1 text-xs">
          <span className="font-semibold">Short title</span>
          <input
            autoFocus
            required
            type="text"
            className="af-input"
            placeholder="e.g. Add rate limiting to /login"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs">
          <span className="font-semibold">Task prompt for agent</span>
          <textarea
            rows={4}
            className="af-input resize-y"
            placeholder="Describe the change, acceptance tests to write, and constraints…"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs">
          <span className="font-semibold">Agent override (optional)</span>
          <select
            className="af-input"
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
          >
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name} {a.installed ? `(${a.version ?? "installed"})` : "(not found)"}
              </option>
            ))}
          </select>
        </label>

        <div className="flex items-center justify-end gap-2 pt-1">
          <button
            type="button"
            className="af-btn"
            onClick={() => setOpen(false)}
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={submitting || !title.trim()}
            className="af-btn af-btn-primary"
          >
            {submitting ? "Creating worktree…" : "+ Add task"}
          </button>
        </div>
      </form>
    </div>
  );
}
