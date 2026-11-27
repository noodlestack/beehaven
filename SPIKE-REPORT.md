# Beehaven Scope S0 — Spike Report

**Date:** November 27, 2026  
**Verdict:** **GO** across all three technical bets and competitive differentiation.

---

## A. Competitive Reality Check (Section 1.6)

Each repository named in Section 1.6 was inspected live on GitHub / `docs.rs` on November 27, 2026:

| Project | URLs Checked | Stack & Surface | License | Pairwise Conflict Matrix (`git merge-tree`)? | Test-Gated Merge Queue? | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| **Grove** | `https://github.com/MichaelVessia/grove` & `/issues` | Rust + FrankenTUI, `tmux`, `lazygit` | MIT | **No** | **No** (manual single-branch `m` merge) | 14 open issues; TUI-only, requires `tmux`. |
| **cwt** | `https://docs.rs/crate/cwt/latest`, `https://github.com/0dragosh/cwt` & `/issues` | Rust `ratatui` TUI, `zellij`/`tmux` | MIT | **No** | **No** (delegates to `gh pr create`) | 3 open issues; ships `.patch` backup before deletion. |
| **workz** | `https://docs.rs/crate/workz/latest`, `https://github.com/rohansx/workz` & `/issues` | Rust CLI + MCP server (TUI removed in `v0.9.0`) | MIT | **No** (`workz conflicts` only checks dirty-file name overlap, not `git merge-tree`) | **No** | 5 open issues; focuses on symlink/port/DB isolation. |
| **Tend** | `https://github.com/knitli/tend` & `/issues` | Tauri 2 + Svelte 5 + `portable-pty` + `xterm.js` | MIT OR Apache-2.0 | **No** | **No** | **Archived (read-only) on June 16, 2026.** |
| **Polypore** | `https://github.com/evanklem/polypore` & `/issues` | Rust `ratatui` TUI + `vt100` + `portable-pty` | MIT | **No** | **No** (prompt queue only) | 3 open issues; TUI rather than desktop GUI. |
| **Crucible** | `https://github.com/The-R4V3N/Crucible` | Unreachable (HTTP 404) | N/A | N/A | N/A | Repository removed or private. |

**Conclusion:** None of the surveyed tools ship a **pairwise cross-branch `git merge-tree` conflict matrix** or a **local test-gated sequential merge queue**, and the closest desktop workbench (`Tend`) has been archived while `Grove`/`cwt`/`Polypore` are terminal TUIs (several requiring `tmux`/`zellij`). Antfarm's Windows/macOS/Linux native Tauri 2 cockpit + ConPTY + conflict matrix + test-gated merge queue has a clear, uncontested differentiator.

---

## B. Terminal Spike (`portable-pty` + Windows ConPTY + `xterm.js`)

- **What was tested:** `SessionManager` (`src-tauri/src/session/mod.rs`) spawning interactive commands on Windows ConPTY (`portable-pty 0.9.0`), writing stdin, resizing (`PtySize`), flooding 2,000 lines through a 16 ms async batching channel, replaying a 256 KB ring buffer after reconnect, and killing a parent + child process tree via `taskkill /PID <pid> /T /F`.
- **Key findings / surprises:**
  1. On Windows ConPTY, `pair.slave` **must** be dropped immediately in the parent process after `spawn_command`; otherwise `reader.read()` blocks past child termination instead of receiving EOF.
  2. **Windows 11 ConPTY DSR (`\x1b[6n`) Startup Handshake:** Windows 11 `conhost` / `OpenConsole` emits an ANSI Device Status Report query (`\x1b[6n`) on startup and blocks child console initialization until a cursor report (`\x1b[1;1R`) is written back to the PTY master. When tasks start in the background without an active `xterm.js` view mounted yet, nobody would answer `\x1b[6n`. `SessionManager` now intercepts `\x1b[6n`, replies `\x1b[1;1R` immediately, and strips `\x1b[6n` from the replay buffer so `xterm.js` never emits a duplicate cursor report on reload.
  3. **ConPTY `ClosePseudoConsole` Flush Window:** Dropping `MasterPty` immediately upon `child.wait()` calls `ClosePseudoConsole`, which discards unread pipe frames still in flight from `conhost.exe`. Retaining `MasterPty` for a short drain window (180 ms) and snapshotting the ring buffer into `completed_replays` preserves 100% of fast-exiting command output.

---

## C. Conflict Forecast Spike (`git merge-tree --write-tree -z`)

- **What was tested:** `forecast_pair` (`src-tauri/src/git/conflict.rs`) against Git `2.50.1.windows.1` on temporary repositories with clean branches, conflicting edits to `shared.txt`, and invalid branch refs.
- **Key finding:**
  - Exit status `0` reliably identifies clean three-way merges.
  - Exit status `1` emits `<tree-oid>\0` followed by NUL-separated `<mode> <oid> <stage>\t<path>\0` records terminated by an empty NUL record (`\0\0`) before human-readable informational messages.
  - Invalid refs on Git `2.50.1.windows.1` exit with code `1` (`merge-tree: not something we can merge`) and empty `stdout`! Verifying that `stdout` begins with a valid 40+ hex character `<tree-oid>\0` distinguishes real conflicts from `MergeTreeOutcome::Error`, ensuring invalid refs are never mistaken for `Clean` or zero-file conflicts.

---

## D. Waiting-For-You Detection Spike (Local Authenticated Hook Socket)

- **What was tested:** `start_hook_listener` (`src-tauri/src/agents/mod.rs`) binding `127.0.0.1:0` with a random 256-bit token, rejecting forged tokens, and receiving `Notification` (`-> WaitingForYou`) and `Stop` (`-> NeedsReview`) hook messages from both a simulated PTY agent (`tests/fixtures/fake-agent.mjs`) and Antfarm's built-in `--antfarm-hook` fast CLI helper wired into `.claude/settings.local.json` (excluded via `git rev-parse --git-path info/exclude` so linked worktrees where `.git` is a file work seamlessly).
- **Fallback:** Output-silence timer (`silence_timeout_secs`, default 20 s) plus one-click manual status toggle for generic CLIs without hooks.
