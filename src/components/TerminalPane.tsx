import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { api, isTauriRuntime } from "../lib/ipc";
import { designTokens, resolveEffectiveTheme } from "../lib/tokens";
import { useAppStore } from "../store/useAppStore";
import type { SessionOutputPayload, Task } from "../types/models";

interface TerminalPaneProps {
  task: Task;
}

export function TerminalPane({ task }: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const settings = useAppStore((s) => s.settings);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const mode = resolveEffectiveTheme(settings.theme);
    const palette = designTokens.themes[mode];

    const term = new Terminal({
      cursorBlink: false, // Calm by default — no blinking status objects (Section 3.1)
      fontFamily: designTokens.typography.fontFamilyMono,
      fontSize: 13,
      lineHeight: 1.35,
      scrollback: 5000,
      screenReaderMode: settings.screen_reader_mode,
      theme: {
        background: palette.background,
        foreground: palette.text,
        cursor: palette.accent,
        selectionBackground: mode === "dark" ? "#334155" : "#CBD5E1",
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(el);

    let disposed = false;

    const safeFitAndResize = () => {
      if (disposed || el.clientWidth <= 0 || el.clientHeight <= 0) return;
      try {
        // Guard against calling fit() before xterm's renderService dimensions initialize
        const core = (term as unknown as { _core?: { _renderService?: { dimensions?: unknown } } })._core;
        if (!core?._renderService?.dimensions) return;
        fitAddon.fit();
        void api.sessionResize(task.id, term.cols, term.rows);
      } catch {
        // Ignore transient layout fit errors
      }
    };

    const rafId = window.requestAnimationFrame(() => {
      if (disposed) return;
      if (isTauriRuntime()) {
        try {
          const webgl = new WebglAddon();
          webgl.onContextLoss(() => webgl.dispose());
          term.loadAddon(webgl);
        } catch {
          // Fallback to standard canvas renderer
        }
      }
      safeFitAndResize();
      // Replay recent rolling buffer from Rust core so UI reloads lose zero output!
      void api.sessionReplay(task.id).then((history) => {
        if (!disposed && history) {
          term.write(history);
        }
      });
    });

    termRef.current = term;
    fitRef.current = fitAddon;

    const dataSub = term.onData((data) => {
      void api.sessionWrite(task.id, data).catch(() => {
        // Session may have already exited
      });
    });

    let unlisten: (() => void) | null = null;
    void api
      .subscribe<SessionOutputPayload>("session:output", (payload) => {
        if (!disposed && payload.task_id === task.id) {
          term.write(payload.chunk);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      });

    const observer = new ResizeObserver(() => {
      safeFitAndResize();
    });
    observer.observe(el);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(rafId);
      observer.disconnect();
      dataSub.dispose();
      if (unlisten) unlisten();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [task.id, settings.theme, settings.screen_reader_mode]);

  const handleRestart = async () => {
    try {
      const updated = await api.taskStart(task.id);
      upsertTaskLocal(updated);
      showToast(`Started session for "${task.title}"`);
    } catch (err) {
      setError(err);
    }
  };

  const handleStop = async () => {
    try {
      const updated = await api.sessionKill(task.id);
      upsertTaskLocal(updated);
      showToast("Stopped terminal session and child process tree.");
    } catch (err) {
      setError(err);
    }
  };

  return (
    <div className="flex h-full flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
        <div className="af-mono truncate" style={{ color: "var(--af-muted)" }}>
          cwd: {task.worktree_path} · agent: {task.agent}
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            className="af-btn"
            onClick={handleRestart}
            title="Start or restart the agent terminal in this worktree"
          >
            ↻ Start / Relaunch
          </button>
          <button
            type="button"
            className="af-btn af-btn-danger"
            onClick={handleStop}
            disabled={task.state !== "Running" && task.state !== "WaitingForYou"}
            title="Terminate the terminal session and its entire process tree"
          >
            ■ Stop session
          </button>
        </div>
      </div>

      <div
        ref={containerRef}
        aria-label={`Embedded terminal for ${task.title}`}
        style={{
          backgroundColor: "var(--af-bg)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="min-h-[340px] flex-1 overflow-hidden border p-2"
      />
    </div>
  );
}
