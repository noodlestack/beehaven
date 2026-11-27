# Beehaven: rules for agents
Project: desktop app (Tauri 2, Rust core, React + TypeScript UI) that runs AI coding
agents in parallel git worktrees, with a conflict forecast and a merge queue.
Read first: the current brief in docs/SCOPES/, DECISIONS.md, design/tokens.json.

Commands
- Rust (from src-tauri/): cargo check, cargo clippy -- -D warnings, cargo test
- UI: pnpm lint, pnpm typecheck, pnpm test
- App: pnpm tauri dev

Rules
1. Work only on the current scope. Do not refactor unrelated code.
2. Write a plan first and wait for approval before coding.
3. All git state changes go through the git module using the git CLI.
   Never run git clean, git reset --hard, or a force push.
4. The web layer never runs shell commands. Every action is a typed Rust command
   with validated, canonicalized paths.
5. No new dependency without a DECISIONS.md entry that includes the scorecard.
6. Every git bug fix comes with a regression test on a temporary repository.
7. Status is never color alone: always a shape and a text label.
8. Use tokens from design/tokens.json. No hardcoded colors.
9. Finish by running the checks and reporting their real output.
   Never claim success without it.
10. If unsure of an API, read its documentation or source. Do not guess.
