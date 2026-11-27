import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { Task, TestRun, TestsProgressPayload } from "../types/models";

const EMPTY_RUNS: TestRun[] = [];

interface TestsPaneProps {
  task: Task;
  testCommand: string;
}

export function TestsPane({ task, testCommand }: TestsPaneProps) {
  const runs = useAppStore((s) => s.latestTestRuns[task.id] ?? EMPTY_RUNS);
  const recordTestRunLocal = useAppStore((s) => s.recordTestRunLocal);
  const setError = useAppStore((s) => s.setError);
  const [running, setRunning] = useState(false);
  const [liveOutput, setLiveOutput] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(runs[0]?.id ?? null);

  useEffect(() => {
    void api.testsList(task.id).then((list) => {
      for (const r of list.reverse()) {
        recordTestRunLocal(r);
      }
      if (list[0]) setExpandedId((prev) => prev ?? list[0]!.id);
    });
  }, [task.id]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void api
      .subscribe<TestsProgressPayload>("tests:progress", (p) => {
        if (p.task_id === task.id) {
          setLiveOutput((prev) => prev + p.chunk);
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      if (unlisten) unlisten();
    };
  }, [task.id]);

  const handleRunTests = async () => {
    setRunning(true);
    setLiveOutput("");
    try {
      const run = await api.testsRun(task.id);
      recordTestRunLocal(run);
      setExpandedId(run.id);
    } catch (err) {
      setError(err);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <div
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="flex flex-wrap items-center justify-between gap-3 border p-3"
      >
        <div className="text-xs">
          <div style={{ color: "var(--af-muted)" }}>Configured test command</div>
          <code className="af-mono text-sm font-semibold">{testCommand}</code>
        </div>
        <button
          type="button"
          className="af-btn af-btn-primary"
          disabled={running}
          onClick={() => void handleRunTests()}
        >
          {running ? "◐ Running tests…" : "▶ Run tests in worktree"}
        </button>
      </div>

      {running && (
        <pre
          style={{
            backgroundColor: "var(--af-bg)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="af-mono m-0 max-h-60 overflow-auto border p-3 text-xs"
        >
          {liveOutput || "Starting test process…"}
        </pre>
      )}

      {runs.length === 0 && !running ? (
        <div className="p-4 text-sm" style={{ color: "var(--af-muted)" }}>
          No test runs recorded for this task yet. Press “Run tests in worktree” to verify its changes.
        </div>
      ) : (
        <div className="flex flex-col gap-2 overflow-y-auto">
          {runs.map((r) => {
            const expanded = expandedId === r.id;
            return (
              <div
                key={r.id}
                style={{
                  backgroundColor: "var(--af-surface)",
                  borderColor: "var(--af-border)",
                  borderRadius: "var(--af-radius-panel)",
                }}
                className="border"
              >
                <button
                  type="button"
                  onClick={() => setExpandedId(expanded ? null : r.id)}
                  className="flex w-full items-center justify-between gap-3 bg-transparent px-3 py-2.5 text-left text-xs cursor-pointer"
                  style={{ color: "var(--af-text)" }}
                >
                  <span className="flex items-center gap-2 font-semibold">
                    <span
                      style={{
                        color: r.passed ? "var(--af-success)" : "var(--af-danger)",
                      }}
                    >
                      {r.passed ? "✔ Passed" : "✗ Failed"}
                    </span>
                    <code className="af-mono" style={{ color: "var(--af-muted)" }}>
                      commit {r.commit_sha.slice(0, 8)}
                    </code>
                  </span>
                  <span className="af-mono" style={{ color: "var(--af-muted)" }}>
                    {r.duration_ms} ms · {expanded ? "Hide logs ▴" : "Show logs ▾"}
                  </span>
                </button>
                {expanded && (
                  <div
                    className="border-t p-3"
                    style={{ borderColor: "var(--af-border)" }}
                  >
                    <div
                      className="af-mono mb-1.5 text-[11px]"
                      style={{ color: "var(--af-muted)" }}
                    >
                      Log file: {r.log_path}
                    </div>
                    <pre
                      style={{
                        backgroundColor: "var(--af-bg)",
                        borderRadius: "var(--af-radius-control)",
                      }}
                      className="af-mono m-0 max-h-64 overflow-auto p-2.5 text-xs whitespace-pre-wrap"
                    >
                      {r.output_preview || "(No stdout/stderr output)"}
                    </pre>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
