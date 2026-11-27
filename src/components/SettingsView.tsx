import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { TaskEvent } from "../types/models";

export function SettingsView() {
  const settings = useAppStore((s) => s.settings);
  const agents = useAppStore((s) => s.agents);
  const updateSettings = useAppStore((s) => s.updateSettings);
  const setOnboardingOpen = useAppStore((s) => s.setOnboardingOpen);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const [draft, setDraft] = useState(settings);
  const [events, setEvents] = useState<TaskEvent[]>([]);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  useEffect(() => {
    void api.eventsList(30).then(setEvents).catch(() => {});
  }, []);

  const handleSave = async () => {
    await updateSettings(draft);
    showToast("Settings saved.");
  };

  const handleExportDiagnostics = async () => {
    try {
      const path = await api.diagnosticsExport();
      showToast(`Exported redacted diagnostics bundle to ${path}`);
    } catch (err) {
      setError(err);
    }
  };

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-5 overflow-y-auto p-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h2 className="m-0 text-xl font-semibold">Settings & Diagnostics</h2>
          <p className="m-0 mt-1 text-xs" style={{ color: "var(--af-muted)" }}>
            Configure concurrency limits, accessibility preferences, themes, and redacted diagnostic exports. No telemetry is ever collected.
          </p>
        </div>
        <button
          type="button"
          className="af-btn"
          onClick={() => setOnboardingOpen(true)}
        >
          Open repository wizard…
        </button>
      </div>

      <div
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="flex flex-col gap-4 border p-4"
      >
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">Max concurrent running agents</span>
            <input
              type="number"
              min={1}
              max={16}
              className="af-input"
              value={draft.max_concurrent_agents}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  max_concurrent_agents: Math.max(1, Number(e.target.value) || 1),
                })
              }
            />
          </label>

          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">
              Generic CLI output-silence warning (seconds)
            </span>
            <input
              type="number"
              min={0}
              max={600}
              className="af-input"
              value={draft.silence_timeout_secs}
              onChange={(e) =>
                setDraft({
                  ...draft,
                  silence_timeout_secs: Math.max(0, Number(e.target.value) || 0),
                })
              }
            />
          </label>

          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">Color theme</span>
            <select
              className="af-input"
              value={draft.theme}
              onChange={(e) => setDraft({ ...draft, theme: e.target.value })}
            >
              <option value="system">Follow system setting</option>
              <option value="dark">Dark theme</option>
              <option value="light">Light theme</option>
            </select>
          </label>

          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">Default coding agent</span>
            <select
              className="af-input"
              value={draft.default_agent}
              onChange={(e) =>
                setDraft({ ...draft, default_agent: e.target.value })
              }
            >
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name} {a.installed ? `(${a.version ?? "installed"})` : "(not detected)"}
                </option>
              ))}
            </select>
          </label>
        </div>

        <label className="flex items-center gap-2.5 text-xs cursor-pointer">
          <input
            type="checkbox"
            checked={draft.screen_reader_mode}
            onChange={(e) =>
              setDraft({ ...draft, screen_reader_mode: e.target.checked })
            }
          />
          <span>
            Enable <strong>xterm.js Screen-Reader Mode</strong> for accessible terminal reading
          </span>
        </label>

        <div className="flex flex-wrap items-center justify-between gap-2 pt-2">
          <button
            type="button"
            className="af-btn af-btn-primary"
            onClick={() => void handleSave()}
          >
            Save settings
          </button>
          <button
            type="button"
            className="af-btn"
            onClick={() => void handleExportDiagnostics()}
          >
            Export diagnostics (prompts redacted)
          </button>
        </div>
      </div>

      {/* Audit Event Log */}
      <div
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="border p-4"
      >
        <h3 className="m-0 text-sm font-semibold">Recent State Machine Audit Log</h3>
        <div className="af-mono mt-2 max-h-56 overflow-y-auto text-xs">
          {events.length === 0 ? (
            <div style={{ color: "var(--af-muted)" }}>No events recorded yet.</div>
          ) : (
            events.map((ev) => (
              <div
                key={ev.id}
                className="border-t py-1.5"
                style={{ borderColor: "var(--af-border)" }}
              >
                <span style={{ color: "var(--af-muted)" }}>
                  [{ev.at.slice(11, 19)}]
                </span>{" "}
                <strong>{ev.kind}</strong> ({ev.task_id.slice(0, 8)}): {ev.detail_json}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
