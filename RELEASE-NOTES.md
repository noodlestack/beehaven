# Beehaven v0.1.0 — Release Notes

**Release Date:** November 27, 2026  
**Repository:** `https://github.com/noodlestack/beehaven`  
**Tag:** `v0.1.0`

## What's Inside Beehaven v0.1.0

- **Isolated Honeycomb Worktrees:** Automatically spins up `<repo-parent>/<repo-name>.worktrees/<id>-<slug>` on branch `beehaven/<id>-<slug>` so parallel coding agents never overwrite each other. Writes full `.patch` backups (tracked + untracked files) before any discard.
- **1-Click Interactive Practice Sandbox:** First-time users can click **Create Practice Sandbox** on the welcome screen or setup wizard to spin up a real local Git repo with 4 worktrees, diffs, passing tests, and a live merge conflict ready to explore immediately.
- **Persistent Embedded Terminals (`portable-pty` + `xterm.js`):** Rust-owned ConPTY/PTY sessions with 16 ms output batching, 256 KB rolling replay across UI reloads, Windows 11 `\x1b[6n` DSR startup handshaking, WebGL rendering, Screen-Reader Mode, and full process-tree termination.
- **Priority Board & Waiting-For-You Detection:** Nine-state SQLite WAL store with crash reconciliation, concurrency slot limiter (default 4), **Claude Code** local hook adapter (`Notification` & `Stop` events over an authenticated local socket), and **Generic CLI** adapter with silence detection + manual status controls.
- **Single-Screen Review (Diff + Tests):** Lazy per-file Unified & Side-by-Side diff viewer against `merge-base`, plus streaming test execution per commit SHA.
- **Honeycomb Conflict Forecast (`git merge-tree --write-tree -z`):** Simulates pairwise 3-way merges across all active task branches without touching working files, caches by commit SHA pair, lists exact colliding files, dispatches one-click resolution prompts to agents, and suggests the cleanest merge order.
- **Test-Gated Merge Queue:** Sequentially rebases approved branches onto `main`, runs your test command in an isolated temporary verification worktree, and fast-forwards (`git merge --ff-only`) only when green—halting safely with `git rebase --abort` if anything fails.

## Installers

- **Windows Setup (NSIS):** `Beehaven_0.1.0_x64-setup.exe`
- **Windows MSI:** `Beehaven_0.1.0_x64_en-US.msi`
