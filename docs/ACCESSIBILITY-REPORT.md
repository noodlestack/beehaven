# Beehaven Accessibility Audit Report (Section 3.6 & Scope S7)

Verified on November 27, 2026 against WCAG 2.1 AA criteria and Section 3.6 of the Build Plan.

| Checklist Item (Section 3.6) | Implementation | Verification Result |
| --- | --- | --- |
| **Status is never color alone** | `StatusBadge.tsx` always renders a distinct geometric shape (`○`, `◐`, `▲`, `◆`, `✓`, `⟳`, `✔`, `✗`, `⊘`) + explicit text label + `role="status"` + `aria-label` alongside semantic token color. Conflict cells render `✓ clean` vs `✗ N files`. | **PASS** (Verified by Vitest `app.test.tsx` & browser inspection) |
| **WCAG AA contrast (`>= 4.5:1` text, `>= 3.0:1` UI/borders/focus)** | Computed programmatically by `scripts/check-contrast.mjs` across all 24 foreground/surface pairs in both `dark` and `light` themes (`docs/STYLE-GUIDE.md`). Minimum text contrast: **4.89:1** (`success` on `surfaceElevated` light); primary text contrast: **14.62:1** (dark) / **15.30:1** (light); borders: **3.80:1**–**4.78:1**. | **PASS** (`node scripts/check-contrast.mjs` exit code `0`) |
| **Keyboard reachability & visible focus rings** | Every interactive control is a native `<button>`, `<select>`, `<input>`, or `<textarea>` with a `2px solid var(--af-accent)` `:focus-visible` ring (`> 6.5:1` contrast). Global shortcuts: `Ctrl+K` / `Cmd+K` (Command Palette), `Alt+N` (Add task), `Alt+1..4` (Screens), `ArrowUp`/`ArrowDown`/`Enter`/`Escape` inside modals and palette. | **PASS** |
| **Calm by default & `prefers-reduced-motion`** | Terminal cursor blink disabled (`cursorBlink: false`); zero flashing or auto-bouncing indicators; `@media (prefers-reduced-motion: reduce)` collapses all animation/transition durations in `src/styles/app.css`. | **PASS** |
| **Icons & meaningful controls have text alternatives** | All icon-prefixed buttons include visible text labels or `aria-label` attributes (`aria-label="Open command palette (Ctrl+K)"`, `aria-label="Move <task> earlier"`, etc.). | **PASS** |
| **Text resize resilience** | Fluid flex/grid layout (`min-h-0`, `flex-wrap`, `overflow-auto`) preserves usability at 125%–200% zoom without clipped actions. | **PASS** |
| **xterm.js Screen-Reader Mode** | Exposed in Settings (`Enable xterm.js Screen-Reader Mode`) and wired directly to `new Terminal({ screenReaderMode })` in `TerminalPane.tsx`. | **PASS** |
