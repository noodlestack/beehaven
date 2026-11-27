use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::agents::detect_all_agents;
use crate::error::AppResult;
use crate::git::safety::check_git_version;
use crate::store::Store;
use crate::time_util::now_rfc3339;

/// Initializes rotating structured logging into `<app_data_dir>/logs/beehaven.log` (Section 4.9).
/// Prompts are deliberately omitted from logs for privacy (Section 4.7).
pub fn init_logging(logs_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    if std::fs::create_dir_all(logs_dir).is_err() {
        return None;
    }
    let file_appender = tracing_appender::rolling::daily(logs_dir, "beehaven.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .try_init();
    Some(guard)
}

#[derive(Debug, Serialize)]
pub struct RedactedTaskSummary {
    id: String,
    repo_id: String,
    branch: String,
    agent: String,
    state: String,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
    // NOTE: `prompt` and `title` are intentionally omitted or redacted to protect user secrets!
}

#[derive(Debug, Serialize)]
pub struct DiagnosticsBundle {
    pub exported_at: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub git_version: String,
    pub git_version_ok: bool,
    pub detected_agents: Vec<crate::models::AgentInfo>,
    pub settings: crate::models::AppSettings,
    pub redacted_tasks: Vec<RedactedTaskSummary>,
    pub recent_events: Vec<crate::models::TaskEvent>,
    pub recent_log_tail: String,
}

/// Writes a redacted JSON diagnostics bundle with prompts stripped (Section 4.7 / 4.9 / S7).
pub fn export_diagnostics_bundle(
    store: &Store,
    logs_dir: &Path,
    destination: &Path,
) -> AppResult<PathBuf> {
    let (git_version, git_version_ok) =
        check_git_version().unwrap_or_else(|_| ("missing".to_string(), false));
    let detected_agents = detect_all_agents();
    let settings = store.get_settings()?;
    let tasks = store.list_all_tasks()?;

    let redacted_tasks = tasks
        .into_iter()
        .map(|t| RedactedTaskSummary {
            id: t.id,
            repo_id: t.repo_id,
            branch: t.branch,
            agent: t.agent,
            state: t.state.as_str().to_string(),
            last_error: t.last_error,
            created_at: t.created_at,
            updated_at: t.updated_at,
        })
        .collect();

    let recent_events = store.list_recent_events(200)?;

    let mut recent_log_tail = String::new();
    if let Ok(entries) = std::fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("beehaven.log") {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    let tail: Vec<&str> = text.lines().rev().take(200).collect();
                    for line in tail.into_iter().rev() {
                        recent_log_tail.push_str(line);
                        recent_log_tail.push('\n');
                    }
                }
            }
        }
    }

    let bundle = DiagnosticsBundle {
        exported_at: now_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        git_version,
        git_version_ok,
        detected_agents,
        settings,
        redacted_tasks,
        recent_events,
        recent_log_tail,
    };

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(destination, serde_json::to_string_pretty(&bundle)?)?;
    Ok(destination.to_path_buf())
}
