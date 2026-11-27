import { getStatusMeta } from "../lib/tokens";
import type { TaskState } from "../types/models";

interface StatusBadgeProps {
  state: TaskState;
  compact?: boolean;
}

const roleColorVar: Record<string, string> = {
  muted: "var(--af-muted)",
  info: "var(--af-info)",
  attention: "var(--af-attention)",
  success: "var(--af-success)",
  danger: "var(--af-danger)",
};

/**
 * Accessible status indicator (Section 3.3 & 3.6):
 * NEVER relies on color alone — always pairs a distinct geometric mark + text label + semantic color.
 */
export function StatusBadge({ state, compact = false }: StatusBadgeProps) {
  const meta = getStatusMeta(state);
  const color = roleColorVar[meta.role] ?? "var(--af-muted)";
  const isAttention = state === "WaitingForYou";

  return (
    <span
      role="status"
      aria-label={`Status: ${meta.label}`}
      style={{
        color,
        borderColor: isAttention ? color : "var(--af-border)",
        backgroundColor: "var(--af-surface-elevated)",
        borderRadius: "var(--af-radius-control)",
      }}
      className={`inline-flex shrink-0 whitespace-nowrap items-center gap-1.5 border font-medium select-none ${
        compact ? "px-1.5 py-0.5 text-xs" : "px-2.5 py-1 text-xs"
      }`}
    >
      <span aria-hidden="true" className="af-mono text-sm leading-none">
        {meta.mark}
      </span>
      <span>{meta.label}</span>
    </span>
  );
}
