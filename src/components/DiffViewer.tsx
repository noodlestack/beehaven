import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { DiffSummary, FileDiff, Task } from "../types/models";

interface DiffViewerProps {
  task: Task;
}

export function DiffViewer({ task }: DiffViewerProps) {
  const [summary, setSummary] = useState<DiffSummary | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileDiff, setFileDiff] = useState<FileDiff | null>(null);
  const [mode, setMode] = useState<"unified" | "split">("unified");
  const [loading, setLoading] = useState(false);
  const setError = useAppStore((s) => s.setError);

  const loadSummary = async () => {
    setLoading(true);
    try {
      const s = await api.diffSummaryGet(task.id);
      setSummary(s);
      const first = s.files[0]?.path ?? null;
      setSelectedFile((prev) =>
        prev && s.files.some((f) => f.path === prev) ? prev : first
      );
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadSummary();
  }, [task.id, task.updated_at]);

  useEffect(() => {
    if (!selectedFile) {
      setFileDiff(null);
      return;
    }
    let cancelled = false;
    void api
      .diffFileGet(task.id, selectedFile)
      .then((d) => {
        if (!cancelled) setFileDiff(d);
      })
      .catch((err) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [task.id, selectedFile, setError]);

  if (loading && !summary) {
    return (
      <div className="p-4 text-sm" style={{ color: "var(--af-muted)" }}>
        Loading changes against base branch…
      </div>
    );
  }

  if (!summary || summary.files.length === 0) {
    return (
      <div className="flex h-full flex-col items-start justify-center gap-3 p-6">
        <p className="m-0 text-sm" style={{ color: "var(--af-muted)" }}>
          No file changes detected yet relative to{" "}
          <code className="af-mono">{summary?.base_branch ?? "base"}</code>.
        </p>
        <button type="button" className="af-btn" onClick={() => void loadSummary()}>
          ↻ Refresh changes
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
        <div className="flex items-center gap-3">
          <span>
            <strong>{summary.files.length}</strong> changed file(s) vs{" "}
            <code className="af-mono">{summary.base_branch}</code>
          </span>
          <span className="af-mono" style={{ color: "var(--af-success)" }}>
            +{summary.total_additions} added
          </span>
          <span className="af-mono" style={{ color: "var(--af-danger)" }}>
            -{summary.total_deletions} removed
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className={`af-btn ${mode === "unified" ? "af-btn-primary" : ""}`}
            aria-pressed={mode === "unified"}
            onClick={() => setMode("unified")}
          >
            Unified
          </button>
          <button
            type="button"
            className={`af-btn ${mode === "split" ? "af-btn-primary" : ""}`}
            aria-pressed={mode === "split"}
            onClick={() => setMode("split")}
          >
            Side-by-side
          </button>
          <button type="button" className="af-btn" onClick={() => void loadSummary()}>
            ↻ Refresh
          </button>
        </div>
      </div>

      <div className="grid min-h-[340px] flex-1 grid-cols-1 gap-3 md:grid-cols-[260px_1fr]">
        {/* File List */}
        <nav
          aria-label="Changed files"
          style={{
            backgroundColor: "var(--af-surface)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="overflow-y-auto border p-2"
        >
          <ul role="list" className="m-0 flex list-none flex-col gap-1 p-0">
            {summary.files.map((f) => {
              const active = f.path === selectedFile;
              return (
                <li key={f.path}>
                  <button
                    type="button"
                    aria-label={`${f.status} ${f.path} (+${f.additions} -${f.deletions})`}
                    onClick={() => setSelectedFile(f.path)}
                    style={{
                      backgroundColor: active
                        ? "var(--af-surface-elevated)"
                        : "transparent",
                      borderColor: active ? "var(--af-accent)" : "transparent",
                      borderRadius: "var(--af-radius-control)",
                    }}
                    className="flex w-full items-center justify-between gap-2 border px-2.5 py-1.5 text-left text-xs cursor-pointer"
                  >
                    <span className="af-mono truncate" title={f.path}>
                      [{f.status}] {f.path}
                    </span>
                    <span className="af-mono shrink-0 text-[11px]">
                      <span style={{ color: "var(--af-success)" }}>+{f.additions}</span>{" "}
                      <span style={{ color: "var(--af-danger)" }}>-{f.deletions}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        {/* Diff Hunk Table */}
        <div
          style={{
            backgroundColor: "var(--af-bg)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-panel)",
          }}
          className="overflow-auto border"
        >
          {!fileDiff ? (
            <div className="p-4 text-xs" style={{ color: "var(--af-muted)" }}>
              Select a file to inspect its diff.
            </div>
          ) : (
            <div>
              <div
                style={{
                  backgroundColor: "var(--af-surface-elevated)",
                  borderBottomColor: "var(--af-border)",
                }}
                className="sticky top-0 flex items-center justify-between border-b px-3 py-2 text-xs"
              >
                <span className="af-mono font-semibold">{fileDiff.path}</span>
                {fileDiff.truncated && (
                  <span style={{ color: "var(--af-attention)" }}>
                    ▲ Truncated to first 8,000 lines for responsiveness
                  </span>
                )}
              </div>

              {fileDiff.hunks.map((hunk, hIdx) => (
                <div key={`${hunk.header}-${hIdx}`}>
                  <div
                    style={{
                      backgroundColor: "var(--af-surface)",
                      color: "var(--af-info)",
                    }}
                    className="af-mono px-3 py-1 text-xs"
                  >
                    {hunk.header}
                  </div>
                  {mode === "unified" ? (
                    <table className="af-mono w-full border-collapse text-xs">
                      <tbody>
                        {hunk.lines.map((ln, lIdx) => {
                          const prefix =
                            ln.kind === "add" ? "+" : ln.kind === "del" ? "-" : " ";
                          const fg =
                            ln.kind === "add"
                              ? "var(--af-success)"
                              : ln.kind === "del"
                              ? "var(--af-danger)"
                              : "var(--af-text)";
                          return (
                            <tr key={lIdx}>
                              <td
                                className="w-12 select-none px-2 py-0.5 text-right"
                                style={{ color: "var(--af-muted)" }}
                              >
                                {ln.old_lineno ?? ""}
                              </td>
                              <td
                                className="w-12 select-none px-2 py-0.5 text-right"
                                style={{ color: "var(--af-muted)" }}
                              >
                                {ln.new_lineno ?? ""}
                              </td>
                              <td
                                className="w-6 select-none px-1 py-0.5 text-center font-bold"
                                style={{ color: fg }}
                              >
                                {prefix}
                              </td>
                              <td
                                className="whitespace-pre-wrap break-all px-2 py-0.5"
                                style={{ color: fg }}
                              >
                                {ln.content}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  ) : (
                    <table className="af-mono w-full table-fixed border-collapse text-xs">
                      <tbody>
                        {hunk.lines.map((ln, lIdx) => (
                          <tr key={lIdx}>
                            <td
                              className="w-10 select-none px-2 py-0.5 text-right"
                              style={{ color: "var(--af-muted)" }}
                            >
                              {ln.old_lineno ?? ""}
                            </td>
                            <td
                              className="w-1/2 whitespace-pre-wrap break-all border-r px-2 py-0.5"
                              style={{
                                borderColor: "var(--af-border)",
                                color:
                                  ln.kind === "del"
                                    ? "var(--af-danger)"
                                    : "var(--af-text)",
                              }}
                            >
                              {ln.kind !== "add"
                                ? `${ln.kind === "del" ? "- " : "  "}${ln.content}`
                                : ""}
                            </td>
                            <td
                              className="w-10 select-none px-2 py-0.5 text-right"
                              style={{ color: "var(--af-muted)" }}
                            >
                              {ln.new_lineno ?? ""}
                            </td>
                            <td
                              className="w-1/2 whitespace-pre-wrap break-all px-2 py-0.5"
                              style={{
                                color:
                                  ln.kind === "add"
                                    ? "var(--af-success)"
                                    : "var(--af-text)",
                              }}
                            >
                              {ln.kind !== "del"
                                ? `${ln.kind === "add" ? "+ " : "  "}${ln.content}`
                                : ""}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
