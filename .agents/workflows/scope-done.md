---
description: Run all Rust, TypeScript, security, and accessibility checks for the current scope and produce a verification summary.
---

# /scope-done Workflow

1. From `src-tauri/`, run:
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`
   - `cargo audit`
   - `cargo deny check`
2. From the repository root, run:
   - `pnpm lint`
   - `pnpm typecheck`
   - `pnpm test`
   - `pnpm build`
3. Verify all status displays use shape + label + semantic token role (never color alone) and `design/tokens.json` contrast checks pass.
4. Update `DECISIONS.md`, `docs/HILL-CHART.md`, and `README.md`.
5. Commit changes cleanly and tag the scope milestone (`v0.1.0-sX`).
