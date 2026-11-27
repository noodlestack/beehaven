#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Fast-path CLI helper for `.claude/settings.local.json` Notification/Stop hooks:
    // `beehaven --beehaven-hook <port> <token> <task_id> <event>`
    if beehaven_lib::agents::try_handle_cli_hook_from_args() {
        return;
    }
    beehaven_lib::run()
}
