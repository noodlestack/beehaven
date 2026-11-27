# Beehaven Decision Log & Dependency Scorecards

This file tracks all architectural choices and dependency evaluations for **Beehaven** (`github.com/noodlestack/beehaven`, renamed from the initial codename *Antfarm*) using the weighted scorecard (max 300 points; adopt threshold >= 210 / 70%).

## 1. Architecture Decisions

| Date | Decision | Options Considered | Why | Revisit When |
| --- | --- | --- | --- | --- |
| 2026-11-27 | Shell out to `git` CLI (`>= 2.38`) for state-changing & merge-tree commands | `git2`, `gix`, `git` CLI | Matches real git worktree, rebase, and `merge-tree --write-tree -z` behavior 100% including rename detection; enforced via strict subcommand allowlist | Read-only status polling on massive monorepos exceeds 200 ms |
| 2026-11-27 | Store worktrees in sibling `<repo-parent>/<repo-name>.worktrees/<slug>` | `.worktrees/` inside repo, OS temp dir, sibling dir | Prevents Cargo/pnpm/TypeScript workspace root discovery in parent directories from confusing nested worktrees with the outer workspace | User configures a custom `worktree_root` in repo settings |
| 2026-11-27 | Tauri 2 + Rust core owning all state & PTY sessions | Electron, TUI (`ratatui`) | Native OS WebView2/WebKit footprint, ConPTY support on Windows without `tmux`, UI reloads never kill running agents | Identical pixel-level Chromium rendering across Linux/macOS/Windows becomes mandatory |
| 2026-11-27 | Own PTY lifecycle directly with `portable-pty` instead of `tauri-plugin-pty` | `tauri-plugin-pty` (0.2.x), `portable-pty` (0.9.0) | Need Rust-owned session persistence across UI reloads, 16 ms output batching, rolling replay buffers, and full process-tree termination (`taskkill /T /F` / process groups) | `tauri-plugin-pty` reaches 1.0 with built-in replay buffers and process-tree kill |
| 2026-11-27 | Type bridge via `ts-rs` (`12.0.1`) + typed `invoke` wrapper | `tauri-specta` (`2.0.0-rc.29`), `ts-rs` (`12.0.1`), manual TS | `tauri-specta` for Tauri v2 remains in release-candidate churn (`2.0.0-rc.29`), whereas `ts-rs` `12.0.1` is stable, MIT-licensed, MSRV 1.88, with 11.9M recent downloads | `tauri-specta` ships a stable `2.0.0` release |
| 2026-11-27 | Local authenticated loopback TCP listener + helper command for Claude Code hooks, plus output-silence fallback | Named pipe only, Unix domain socket, loopback TCP (`127.0.0.1:0`) with random 256-bit token | Works identically on Windows, macOS, and Linux from `.claude/settings.local.json` (`Notification` and `Stop` hooks) using zero external tools; fallback output-silence timer covers generic CLIs | Platform-specific IPC throughput becomes a bottleneck |
| 2026-11-27 | Diff viewer: structured Rust git unified/split parser + accessible virtualized React viewer (evaluated vs `@monaco-editor/react` & `react-diff-viewer-continued` in S4) | `@monaco-editor/react`, `react-diff-viewer-continued`, `@git-diff-view/react`, native parsed hunk viewer | Antfarm is read-only for diffs (non-goal: IDE/editor); avoiding Monaco saves ~3.5 MB bundle weight, eliminates WebWorker CSP friction, handles 10k+ line diffs via lazy per-file loading & hunk virtualization, and guarantees keyboard + screen-reader semantics | Inline multi-cursor diff editing is added in v2 |
| 2026-11-27 | License project as MIT with zero telemetry and unsigned v1 local installers | MIT, Apache-2.0, GPL-3.0 | Permissive, matches ecosystem (`tauri`, `portable-pty`, `rusqlite`, `ts-rs`), clear privacy posture | Public commercial distribution requires EV code-signing certificates |

---

## 2. Dependency Scorecards

Weights: **Fit (25)** · **Maintenance (20)** · **Security (15)** · **License (10)** · **Adoption (10)** · **Stability (10)** · **Cost of Ownership (10)** = **Max 300** (Adopt >= 210 / 70%).

### 2.1 Rust Dependencies

| Crate | Version | Fit (x25) | Maint (x20) | Sec (x15) | Lic (x10) | Adopt (x10) | Stab (x10) | Cost (x10) | Total / 300 (%) | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `tauri` + `tauri-build` + `tauri-plugin-dialog` | `2.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 2 | **290 (96.7%)** | **Adopt** |
| `portable-pty` | `0.9.0` | 3 | 2 | 3 | 3 (MIT) | 3 (19.2M dl) | 3 | 3 | **280 (93.3%)** | **Adopt** |
| `rusqlite` (`bundled`) | `0.43` / latest `0.3x+` | 3 | 3 (Nov 2026) | 3 | 3 (MIT) | 3 (69.7M dl) | 3 | 2 | **290 (96.7%)** | **Adopt** |
| `ts-rs` | `12.0.1` | 3 | 3 (Feb 2026) | 3 | 3 (MIT) | 3 (11.9M dl) | 3 | 3 | **300 (100%)** | **Adopt** (beats `tauri-specta` `2.0.0-rc.29` at 215/300 due to RC churn) |
| `tauri-specta` | `2.0.0-rc.29` | 3 | 2 | 3 | 3 (MIT) | 2 | 1 (RC only) | 2 | **245 (81.7%)** | **Rejected** in favor of stable `ts-rs` |
| `tokio` | `1.x` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `serde` + `serde_json` | `1.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `thiserror` + `anyhow` | `2.x` / `1.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `tracing` + `tracing-subscriber` + `tracing-appender` | `0.1` / `0.3` / `0.2` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `uuid` (`v4`) | `1.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `chrono` | `0.4` | 3 | 0 (`RUSTSEC-2026-0543` unmaintained) | 1 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **VETOED** | **Removed** after `cargo audit` flagged `RUSTSEC-2026-0543`; replaced with 20-line pure-`std` `src-tauri/src/time_util.rs` |
| `tempfile` (dev) | `3.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **300 (100%)** | **Adopt** (dev-only) |

### 2.2 Frontend Dependencies

| Package | Version | Fit (x25) | Maint (x20) | Sec (x15) | Lic (x10) | Adopt (x10) | Stab (x10) | Cost (x10) | Total / 300 (%) | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `react` + `react-dom` | `19.x` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `@tauri-apps/api` + `@tauri-apps/plugin-dialog` + `@tauri-apps/cli` | `2.x` | 3 | 3 | 3 | 3 (MIT/Apache-2.0) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `@xterm/xterm` + `@xterm/addon-fit` + `@xterm/addon-webgl` | `5.5+` / `0.10+` / `0.18+` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `zustand` | `5.x` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `tailwindcss` + `@tailwindcss/vite` | `4.x` | 3 | 3 | 3 | 3 (MIT) | 3 | 3 | 3 | **300 (100%)** | **Adopt** |
| `@monaco-editor/react` | `4.7.0` | 2 (overkill editor) | 2 (Feb 2025 stable; React 19 needs RC) | 3 | 3 (MIT) | 3 (9.9M/wk) | 2 | 1 (~3.5MB + workers) | **225 (75.0%)** | **Rejected** in S4 trial (heavy bundle, React 19 peer friction, CSP worker overhead) |
| `@git-diff-view/react` | `0.1.8` | 2 | 3 (Oct 2026) | 3 | 3 (MIT) | 2 (236k/wk) | 1 (`0.1.x` pre-1.0) | 2 | **235 (78.3%)** | **Rejected** in favor of zero-extra-dep Rust-parsed unified/split hunk viewer |
| `react-diff-viewer-continued` | `4.4.0` | 2 (re-diffs full files in JS) | 3 (Jul 2026) | 3 | 3 (MIT) | 3 (1.39M/wk) | 2 | 2 | **250 (83.3%)** | **Runner-up**; native Rust `git diff` hunk parser + accessible virtualized React diff table scored **290 (96.7%)** with 0 extra runtime dependencies |
