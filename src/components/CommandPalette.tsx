import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";

interface PaletteItem {
  id: string;
  label: string;
  shortcut?: string;
  action: () => void;
}

export function CommandPalette() {
  const open = useAppStore((s) => s.commandPaletteOpen);
  const setOpen = useAppStore((s) => s.setCommandPaletteOpen);
  const tasks = useAppStore((s) => s.tasks);
  const settings = useAppStore((s) => s.settings);
  const updateSettings = useAppStore((s) => s.updateSettings);
  const setActiveScreen = useAppStore((s) => s.setActiveScreen);
  const selectTask = useAppStore((s) => s.selectTask);
  const setAddTaskOpen = useAppStore((s) => s.setAddTaskOpen);
  const setOnboardingOpen = useAppStore((s) => s.setOnboardingOpen);
  const refreshConflicts = useAppStore((s) => s.refreshConflicts);
  const showToast = useAppStore((s) => s.showToast);

  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
    }
  }, [open]);

  const items = useMemo<PaletteItem[]>(() => {
    const base: PaletteItem[] = [
      {
        id: "add-task",
        label: "+ Add new parallel task",
        shortcut: "Alt+N",
        action: () => setAddTaskOpen(true),
      },
      {
        id: "screen-board",
        label: "Go to Board & Task Inspector",
        shortcut: "Alt+1",
        action: () => setActiveScreen("board"),
      },
      {
        id: "screen-conflicts",
        label: "Go to Pairwise Conflict Matrix",
        shortcut: "Alt+2",
        action: () => setActiveScreen("conflicts"),
      },
      {
        id: "screen-queue",
        label: "Go to Test-Gated Merge Queue",
        shortcut: "Alt+3",
        action: () => setActiveScreen("queue"),
      },
      {
        id: "screen-settings",
        label: "Go to Settings & Diagnostics",
        shortcut: "Alt+4",
        action: () => setActiveScreen("settings"),
      },
      {
        id: "open-repo",
        label: "Open or switch Git repository…",
        action: () => setOnboardingOpen(true),
      },
      {
        id: "recompute-conflicts",
        label: "Recompute pairwise conflict forecast",
        action: () => {
          setActiveScreen("conflicts");
          void refreshConflicts();
        },
      },
      {
        id: "toggle-theme",
        label: "Switch dark / light theme",
        action: () => {
          const nextTheme = settings.theme === "light" ? "dark" : "light";
          void updateSettings({ ...settings, theme: nextTheme });
        },
      },
      {
        id: "export-diag",
        label: "Export redacted diagnostics bundle",
        action: () => {
          void api
            .diagnosticsExport()
            .then((p) => showToast(`Exported diagnostics to ${p}`));
        },
      },
    ];

    const taskJumpers: PaletteItem[] = tasks.map((t) => ({
      id: `jump-${t.id}`,
      label: `Task: ${t.title} (${t.state})`,
      action: () => {
        selectTask(t.id);
        setActiveScreen("board");
      },
    }));

    const all = [...base, ...taskJumpers];
    if (!query.trim()) return all;
    const q = query.toLowerCase();
    return all.filter((it) => it.label.toLowerCase().includes(q));
  }, [
    query,
    tasks,
    settings,
    setAddTaskOpen,
    setActiveScreen,
    setOnboardingOpen,
    refreshConflicts,
    updateSettings,
    showToast,
    selectTask,
  ]);

  if (!open) return null;

  const runSelected = (item: PaletteItem | undefined) => {
    if (!item) return;
    setOpen(false);
    item.action();
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 pt-20 p-4"
      onClick={() => setOpen(false)}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="w-full max-w-lg overflow-hidden border shadow-2xl"
      >
        <div
          className="border-b p-3"
          style={{ borderColor: "var(--af-border)" }}
        >
          <input
            autoFocus
            type="text"
            aria-label="Search commands and tasks"
            className="af-input"
            placeholder="Type a command or task name… (↑/↓ to navigate, Enter to run)"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setOpen(false);
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                setActiveIndex((i) => Math.min(items.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActiveIndex((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                runSelected(items[activeIndex]);
              }
            }}
          />
        </div>
        <ul
          role="listbox"
          className="m-0 max-h-72 list-none overflow-y-auto p-2"
        >
          {items.length === 0 ? (
            <li className="p-3 text-xs" style={{ color: "var(--af-muted)" }}>
              No matching commands or tasks.
            </li>
          ) : (
            items.map((it, idx) => {
              const active = idx === activeIndex;
              return (
                <li key={it.id} role="option" aria-selected={active}>
                  <button
                    type="button"
                    onClick={() => runSelected(it)}
                    onMouseEnter={() => setActiveIndex(idx)}
                    style={{
                      backgroundColor: active
                        ? "var(--af-surface-elevated)"
                        : "transparent",
                      borderColor: active ? "var(--af-accent)" : "transparent",
                      borderRadius: "var(--af-radius-control)",
                    }}
                    className="flex w-full items-center justify-between border px-3 py-2 text-left text-xs cursor-pointer"
                  >
                    <span>{it.label}</span>
                    {it.shortcut && (
                      <kbd
                        className="af-mono text-[11px]"
                        style={{ color: "var(--af-muted)" }}
                      >
                        {it.shortcut}
                      </kbd>
                    )}
                  </button>
                </li>
              );
            })
          )}
        </ul>
      </div>
    </div>
  );
}
