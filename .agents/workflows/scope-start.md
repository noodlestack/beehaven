---
description: Read the current scope brief, check DECISIONS.md and design/tokens.json, and verify assumptions before planning.
---

# /scope-start Workflow

1. Identify the active scope brief in `docs/SCOPES/` (`S0.md` through `S7.md`).
2. Read the active scope brief, `AGENTS.md`, `DECISIONS.md`, and `design/tokens.json`.
3. Inspect existing Rust modules (`src-tauri/src/`) and React UI modules (`src/`) touched by the scope.
4. Check if any new dependency is required; if so, run the 7-step scorecard evaluation and record it in `DECISIONS.md` before adding to `Cargo.toml` or `package.json`.
5. Produce a concrete implementation plan listing modules, IPC commands/events, safety guards, and automated tests covering every acceptance criterion in the scope brief.
