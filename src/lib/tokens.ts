import rawTokens from "../../design/tokens.json";
import type { TaskState } from "../types/models";

export const designTokens = rawTokens;

export type ThemeMode = "dark" | "light";

export function resolveEffectiveTheme(setting: string): ThemeMode {
  if (setting === "light") return "light";
  if (setting === "dark") return "dark";
  if (
    typeof window !== "undefined" &&
    window.matchMedia &&
    window.matchMedia("(prefers-color-scheme: light)").matches
  ) {
    return "light";
  }
  return "dark";
}

export function applyThemeTokens(mode: ThemeMode): void {
  if (typeof document === "undefined") return;
  const palette = designTokens.themes[mode];
  const root = document.documentElement;
  root.dataset.theme = mode;

  root.style.setProperty("--af-font-ui", designTokens.typography.fontFamilyUi);
  root.style.setProperty("--af-font-mono", designTokens.typography.fontFamilyMono);
  root.style.setProperty("--af-radius-control", designTokens.radii.control);
  root.style.setProperty("--af-radius-panel", designTokens.radii.panel);

  root.style.setProperty("--af-bg", palette.background);
  root.style.setProperty("--af-surface", palette.surface);
  root.style.setProperty("--af-surface-elevated", palette.surfaceElevated);
  root.style.setProperty("--af-text", palette.text);
  root.style.setProperty("--af-muted", palette.mutedText);
  root.style.setProperty("--af-border", palette.border);
  root.style.setProperty("--af-accent", palette.accent);
  root.style.setProperty("--af-info", palette.info);
  root.style.setProperty("--af-attention", palette.attention);
  root.style.setProperty("--af-success", palette.success);
  root.style.setProperty("--af-danger", palette.danger);
}

export interface StatusMeta {
  mark: string;
  label: string;
  role: "muted" | "info" | "attention" | "success" | "danger";
}

export function getStatusMeta(state: TaskState): StatusMeta {
  const vocab = designTokens.statusVocabulary[state];
  return {
    mark: vocab.mark,
    label: vocab.label,
    role: vocab.role as StatusMeta["role"],
  };
}
