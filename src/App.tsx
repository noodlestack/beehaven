import { useEffect } from "react";
import { api } from "./lib/ipc";
import { useAppStore } from "./store/useAppStore";
import type {
  ConflictMatrixReport,
  QueueState,
  TaskStatePayload,
  TestRun,
} from "./types/models";
import { AddTaskModal } from "./components/AddTaskModal";
import { BoardView } from "./components/BoardView";
import { CommandPalette } from "./components/CommandPalette";
import { ConflictMatrixView } from "./components/ConflictMatrixView";
import { MergeQueueView } from "./components/MergeQueueView";
import { OnboardingWizard } from "./components/OnboardingWizard";
import { SettingsView } from "./components/SettingsView";

export function App() {
  const init = useAppStore((s) => s.init);
  const repos = useAppStore((s) => s.repos);
  const activeRepoId = useAppStore((s) => s.activeRepoId);
  const selectRepo = useAppStore((s) => s.selectRepo);
  const tasks = useAppStore((s) => s.tasks);
  const conflictReport = useAppStore((s) => s.conflictReport);
  const queue = useAppStore((s) => s.queue);
  const latestTestRuns = useAppStore((s) => s.latestTestRuns);
  const activeScreen = useAppStore((s) => s.activeScreen);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const setAddTaskOpen = useAppStore((s) => s.setAddTaskOpen);
  const setCommandPaletteOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const setOnboardingOpen = useAppStore((s) => s.setOnboardingOpen);
  const bannerError = useAppStore((s) => s.bannerError);
  const clearError = useAppStore((s) => s.clearError);
  const toastMessage = useAppStore((s) => s.toastMessage);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const recordTestRunLocal = useAppStore((s) => s.recordTestRunLocal);
  const setQueueStateLocal = useAppStore((s) => s.setQueueStateLocal);

  useEffect(() => {
    void init();
  }, [init]);

  // Subscribe to Rust core real-time events (Section 4.6)
  useEffect(() => {
    const unsubs: Array<() => void> = [];
    void api
      .subscribe<TaskStatePayload>("task:state", (p) => {
        upsertTaskLocal(p.task);
      })
      .then((u) => unsubs.push(u));
    void api
      .subscribe<TestRun>("tests:done", (r) => {
        recordTestRunLocal(r);
      })
      .then((u) => unsubs.push(u));
    void api
      .subscribe<QueueState>("queue:progress", (q) => {
        setQueueStateLocal(q);
      })
      .then((u) => unsubs.push(u));
    void api
      .subscribe<ConflictMatrixReport>("conflicts:updated", (rep) => {
        useAppStore.setState({ conflictReport: rep });
      })
      .then((u) => unsubs.push(u));

    return () => {
      for (const u of unsubs) u();
    };
  }, [upsertTaskLocal, recordTestRunLocal, setQueueStateLocal]);

  // Global keyboard shortcuts (Section 3.1: Keyboard first, Ctrl/Cmd+K command palette)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setCommandPaletteOpen(!useAppStore.getState().commandPaletteOpen);
      } else if (e.altKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        setAddTaskOpen(true);
      } else if (e.altKey && e.key === "1") {
        e.preventDefault();
        setActiveScreen("board");
      } else if (e.altKey && e.key === "2") {
        e.preventDefault();
        setActiveScreen("conflicts");
      } else if (e.altKey && e.key === "3") {
        e.preventDefault();
        setActiveScreen("queue");
      } else if (e.altKey && e.key === "4") {
        e.preventDefault();
        setActiveScreen("settings");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setCommandPaletteOpen, setAddTaskOpen, setActiveScreen]);

  const activeRepo = repos.find((r) => r.id === activeRepoId) ?? null;
  const waitingCount = tasks.filter((t) => t.state === "WaitingForYou").length;
  const approvedCount = tasks.filter((t) => t.state === "Approved").length;
  const conflictPairsCount = (conflictReport?.cells ?? []).filter(
    (c) => c.status === "Conflicted" || c.status === "Error"
  ).length;

  let passedTests = 0;
  let failedTests = 0;
  for (const t of tasks) {
    const latest = latestTestRuns[t.id]?.[0];
    if (latest) {
      if (latest.passed) passedTests++;
      else failedTests++;
    }
  }

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      {/* Top Header Bar (Section 3.2 Wireframe) */}
      <header
        style={{
          backgroundColor: "var(--af-surface)",
          borderBottomColor: "var(--af-border)",
        }}
        className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-2.5"
      >
        <div className="flex flex-wrap items-center gap-3">
          <button
            type="button"
            onClick={() => setActiveScreen("board")}
            className="flex items-center gap-2 border-0 bg-transparent p-0 text-base font-bold cursor-pointer"
            style={{ color: "var(--af-text)" }}
          >
            <svg
              aria-hidden="true"
              width="20"
              height="20"
              viewBox="0 0 1024 1024"
              className="shrink-0"
            >
              <polygon
                points="512,140 660,225 660,395 512,480 364,395 364,225"
                fill="none"
                stroke="var(--af-attention)"
                strokeWidth="72"
              />
              <polygon
                points="340,440 488,525 488,695 340,780 192,695 192,525"
                fill="none"
                stroke="var(--af-info)"
                strokeWidth="72"
              />
              <polygon
                points="684,440 832,525 832,695 684,780 536,695 536,525"
                fill="none"
                stroke="var(--af-success)"
                strokeWidth="72"
              />
            </svg>
            <span>Beehaven</span>
            {waitingCount > 0 && (
              <span
                aria-label={`${waitingCount} task waiting for you`}
                style={{
                  color: "var(--af-attention)",
                  borderColor: "var(--af-attention)",
                  borderRadius: "var(--af-radius-control)",
                }}
                className="border px-1.5 py-0.5 text-xs font-semibold"
              >
                ▲ {waitingCount} waiting
              </span>
            )}
          </button>

          {repos.length > 0 && (
            <div className="flex items-center gap-2 text-xs">
              <label htmlFor="repo-switcher" style={{ color: "var(--af-muted)" }}>
                repo:
              </label>
              <select
                id="repo-switcher"
                value={activeRepoId ?? ""}
                onChange={(e) => void selectRepo(e.target.value)}
                className="af-input w-auto py-1 font-medium"
              >
                {repos.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </select>
              {activeRepo && (
                <span className="whitespace-nowrap" style={{ color: "var(--af-muted)" }}>
                  base: <code className="af-mono">{activeRepo.base_branch}</code>
                </span>
              )}
            </div>
          )}

          <button
            type="button"
            className="af-btn whitespace-nowrap py-1 text-xs"
            onClick={() => setOnboardingOpen(true)}
          >
            Open repo…
          </button>
        </div>

        {/* Screen Navigation + Actions */}
        <nav aria-label="Primary screens" className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            aria-current={activeScreen === "board" ? "page" : undefined}
            className={`af-btn ${activeScreen === "board" ? "af-btn-primary" : ""}`}
            onClick={() => setActiveScreen("board")}
          >
            Board
          </button>
          <button
            type="button"
            aria-current={activeScreen === "conflicts" ? "page" : undefined}
            className={`af-btn ${activeScreen === "conflicts" ? "af-btn-primary" : ""}`}
            onClick={() => setActiveScreen("conflicts")}
          >
            {conflictPairsCount > 0 ? `Conflicts (${conflictPairsCount})` : "Conflicts"}
          </button>
          <button
            type="button"
            aria-current={activeScreen === "queue" ? "page" : undefined}
            className={`af-btn ${activeScreen === "queue" ? "af-btn-primary" : ""}`}
            onClick={() => setActiveScreen("queue")}
          >
            {approvedCount > 0 ? `Merge Queue (${approvedCount})` : "Merge Queue"}
          </button>
          <button
            type="button"
            aria-current={activeScreen === "settings" ? "page" : undefined}
            className={`af-btn ${activeScreen === "settings" ? "af-btn-primary" : ""}`}
            onClick={() => setActiveScreen("settings")}
          >
            Settings
          </button>
          <button
            type="button"
            className="af-btn af-btn-primary"
            disabled={!activeRepo}
            onClick={() => setAddTaskOpen(true)}
          >
            + Add task
          </button>
          <button
            type="button"
            className="af-btn af-mono text-xs"
            onClick={() => setCommandPaletteOpen(true)}
            aria-label="Open command palette (Ctrl+K)"
          >
            Ctrl+K
          </button>
        </nav>
      </header>

      {/* Plain-Language Error Banner with Details (Section 3.7 & 4.6) */}
      {bannerError && (
        <div
          role="alert"
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderBottomColor: "var(--af-danger)",
          }}
          className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-2.5 text-xs"
        >
          <div>
            <strong style={{ color: "var(--af-danger)" }}>
              ✗ {bannerError.message}
            </strong>
            {bannerError.details && (
              <span className="af-mono ml-2" style={{ color: "var(--af-muted)" }}>
                ({bannerError.details})
              </span>
            )}
          </div>
          <button type="button" className="af-btn py-0.5" onClick={clearError}>
            Dismiss
          </button>
        </div>
      )}

      {/* Non-intrusive Calm Toast */}
      {toastMessage && (
        <div
          role="status"
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderBottomColor: "var(--af-accent)",
          }}
          className="border-b px-4 py-1.5 text-xs"
        >
          {toastMessage}
        </div>
      )}

      {/* Main Content Area */}
      <main className="min-h-0 flex-1 overflow-hidden">
        {activeScreen === "board" && <BoardView />}
        {activeScreen === "conflicts" && <ConflictMatrixView />}
        {activeScreen === "queue" && <MergeQueueView />}
        {activeScreen === "settings" && <SettingsView />}
      </main>

      {/* Bottom Global Summary & Navigation Bar (Section 3.2 Wireframe) */}
      <footer
        style={{
          backgroundColor: "var(--af-surface)",
          borderTopColor: "var(--af-border)",
        }}
        className="flex flex-wrap items-center justify-between gap-4 border-t px-4 py-2 text-xs"
      >
        <div className="flex flex-wrap items-center gap-4">
          <button
            type="button"
            onClick={() => setActiveScreen("queue")}
            className="border-0 bg-transparent p-0 cursor-pointer"
            style={{ color: "var(--af-text)" }}
          >
            <strong>Queue:</strong>{" "}
            {queue.running
              ? "⟳ running"
              : approvedCount > 0
              ? `✓ ${approvedCount} ready to merge`
              : "empty"}
          </button>
          <button
            type="button"
            onClick={() => setActiveScreen("conflicts")}
            className="border-0 bg-transparent p-0 cursor-pointer"
            style={{
              color:
                conflictPairsCount > 0 ? "var(--af-danger)" : "var(--af-text)",
            }}
          >
            <strong>Conflicts:</strong>{" "}
            {conflictPairsCount === 0
              ? "✓ 0 pairs"
              : `✗ ${conflictPairsCount} pair${conflictPairsCount === 1 ? "" : "s"}`}
          </button>
          <span style={{ color: "var(--af-muted)" }}>
            <strong>Tests:</strong> {passedTests} passed, {failedTests} failed
          </span>
        </div>
        <div style={{ color: "var(--af-muted)" }}>
          Local-only · Zero telemetry · Backup patches enabled
        </div>
      </footer>

      <AddTaskModal />
      <OnboardingWizard />
      <CommandPalette />
    </div>
  );
}
