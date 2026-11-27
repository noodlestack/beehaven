use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::error::AppResult;
use crate::git::safety::run_git;
use crate::models::{AgentInfo, Task, TaskState};
use crate::session::CommandSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMessage {
    pub token: String,
    pub task_id: String,
    pub event: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum AgentSignal {
    HookEvent(String),
    SilenceTimeout,
    Exited(Option<i32>),
}

/// Unified interface hiding CLI differences behind one adapter per agent (Section 4.5).
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn supports_hooks(&self) -> bool;
    fn detect(&self) -> Option<String>;
    fn command(&self, task: &Task, hook_port: u16, hook_token: &str) -> CommandSpec;
    fn prepare(
        &self,
        worktree: &Path,
        task: &Task,
        hook_port: u16,
        hook_token: &str,
    ) -> AppResult<()>;
    fn interpret(&self, signal: &AgentSignal) -> Option<TaskState>;
}

pub struct ClaudeCodeAdapter;

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &str {
        "claude"
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn supports_hooks(&self) -> bool {
        true
    }

    fn detect(&self) -> Option<String> {
        let mut cmd = Command::new("claude");
        cmd.arg("--version");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let out = cmd.output().ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    }

    fn command(&self, task: &Task, hook_port: u16, hook_token: &str) -> CommandSpec {
        let mut args = Vec::new();
        if !task.prompt.trim().is_empty() {
            args.push(task.prompt.clone());
        }
        CommandSpec {
            program: "claude".to_string(),
            args,
            env: vec![
                ("BEEHAVEN_TASK_ID".into(), task.id.clone()),
                ("BEEHAVEN_HOOK_PORT".into(), hook_port.to_string()),
                ("BEEHAVEN_HOOK_TOKEN".into(), hook_token.to_string()),
            ],
            cwd: Path::new(&task.worktree_path).to_path_buf(),
        }
    }

    /// Writes `.claude/settings.local.json` with `Notification` and `Stop` hooks pointing to
    /// Beehaven's `--beehaven-hook` helper mode, and excludes `.claude/` via
    /// the worktree's `git rev-parse --git-path info/exclude` so nothing is ever committed!
    fn prepare(
        &self,
        worktree: &Path,
        task: &Task,
        hook_port: u16,
        hook_token: &str,
    ) -> AppResult<()> {
        let claude_dir = worktree.join(".claude");
        std::fs::create_dir_all(&claude_dir)?;

        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| "beehaven".to_string());

        let notify_cmd = format!(
            "\"{exe_path}\" --beehaven-hook {hook_port} {hook_token} {} Notification",
            task.id
        );
        let stop_cmd = format!(
            "\"{exe_path}\" --beehaven-hook {hook_port} {hook_token} {} Stop",
            task.id
        );

        let settings = serde_json::json!({
            "hooks": {
                "Notification": [
                    {
                        "matcher": "",
                        "hooks": [
                            { "type": "command", "command": notify_cmd }
                        ]
                    }
                ],
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [
                            { "type": "command", "command": stop_cmd }
                        ]
                    }
                ]
            }
        });

        std::fs::write(
            claude_dir.join("settings.local.json"),
            serde_json::to_string_pretty(&settings)?,
        )?;

        // Exclude .claude/settings.local.json inside this worktree's gitdir exclude file
        if let Ok(out) = run_git(worktree, &["rev-parse", "--git-path", "info/exclude"]) {
            if out.status.success() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let exclude_path = if Path::new(&raw).is_absolute() {
                    Path::new(&raw).to_path_buf()
                } else {
                    worktree.join(raw)
                };
                if let Some(parent) = exclude_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
                if !existing.contains(".claude/settings.local.json") {
                    let mut updated = existing;
                    if !updated.ends_with('\n') && !updated.is_empty() {
                        updated.push('\n');
                    }
                    updated.push_str(".claude/settings.local.json\n");
                    let _ = std::fs::write(&exclude_path, updated);
                }
            }
        }

        Ok(())
    }

    fn interpret(&self, signal: &AgentSignal) -> Option<TaskState> {
        match signal {
            AgentSignal::HookEvent(ev) if ev.eq_ignore_ascii_case("Notification") => {
                Some(TaskState::WaitingForYou)
            }
            AgentSignal::HookEvent(ev) if ev.eq_ignore_ascii_case("Stop") => {
                Some(TaskState::NeedsReview)
            }
            AgentSignal::HookEvent(_) => None,
            AgentSignal::Exited(Some(0)) => Some(TaskState::NeedsReview),
            AgentSignal::Exited(Some(_)) => Some(TaskState::Failed),
            AgentSignal::Exited(None) => Some(TaskState::NeedsReview),
            AgentSignal::SilenceTimeout => None,
        }
    }
}

pub struct GenericShellAdapter;

impl AgentAdapter for GenericShellAdapter {
    fn id(&self) -> &str {
        "generic"
    }

    fn display_name(&self) -> &str {
        "Interactive Shell / Custom CLI"
    }

    fn supports_hooks(&self) -> bool {
        false
    }

    fn detect(&self) -> Option<String> {
        #[cfg(windows)]
        {
            Some("PowerShell (Built-in)".to_string())
        }
        #[cfg(not(windows))]
        {
            Some(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()))
        }
    }

    fn command(&self, task: &Task, hook_port: u16, hook_token: &str) -> CommandSpec {
        // Allows `custom:<command>` in `task.agent`, otherwise opens interactive OS shell and prints prompt banner.
        if let Some(custom_cmd) = task.agent.strip_prefix("custom:") {
            #[cfg(windows)]
            let (prog, args) = (
                "powershell.exe".to_string(),
                vec![
                    "-NoLogo".to_string(),
                    "-Command".to_string(),
                    custom_cmd.to_string(),
                ],
            );
            #[cfg(not(windows))]
            let (prog, args) = (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), custom_cmd.to_string()],
            );
            return CommandSpec {
                program: prog,
                args,
                env: vec![
                    ("BEEHAVEN_TASK_ID".into(), task.id.clone()),
                    ("BEEHAVEN_PROMPT".into(), task.prompt.clone()),
                    ("BEEHAVEN_HOOK_PORT".into(), hook_port.to_string()),
                    ("BEEHAVEN_HOOK_TOKEN".into(), hook_token.to_string()),
                ],
                cwd: Path::new(&task.worktree_path).to_path_buf(),
            };
        }

        #[cfg(windows)]
        let (program, args) = ("powershell.exe".to_string(), vec!["-NoLogo".to_string()]);
        #[cfg(not(windows))]
        let (program, args) = (
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            vec![],
        );

        CommandSpec {
            program,
            args,
            env: vec![
                ("BEEHAVEN_TASK_ID".into(), task.id.clone()),
                ("BEEHAVEN_PROMPT".into(), task.prompt.clone()),
                ("BEEHAVEN_HOOK_PORT".into(), hook_port.to_string()),
                ("BEEHAVEN_HOOK_TOKEN".into(), hook_token.to_string()),
            ],
            cwd: Path::new(&task.worktree_path).to_path_buf(),
        }
    }

    fn prepare(
        &self,
        _worktree: &Path,
        _task: &Task,
        _hook_port: u16,
        _hook_token: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    fn interpret(&self, signal: &AgentSignal) -> Option<TaskState> {
        match signal {
            AgentSignal::HookEvent(ev) if ev.eq_ignore_ascii_case("Notification") => {
                Some(TaskState::WaitingForYou)
            }
            AgentSignal::HookEvent(ev) if ev.eq_ignore_ascii_case("Stop") => {
                Some(TaskState::NeedsReview)
            }
            AgentSignal::SilenceTimeout => Some(TaskState::WaitingForYou),
            AgentSignal::HookEvent(_) => None,
            AgentSignal::Exited(Some(0)) => Some(TaskState::NeedsReview),
            AgentSignal::Exited(Some(_)) => Some(TaskState::Failed),
            AgentSignal::Exited(None) => Some(TaskState::NeedsReview),
        }
    }
}

pub fn get_adapter(agent_id: &str) -> Box<dyn AgentAdapter> {
    if agent_id == "claude" {
        Box::new(ClaudeCodeAdapter)
    } else {
        Box::new(GenericShellAdapter)
    }
}

pub fn detect_all_agents() -> Vec<AgentInfo> {
    let adapters: Vec<Box<dyn AgentAdapter>> =
        vec![Box::new(ClaudeCodeAdapter), Box::new(GenericShellAdapter)];
    adapters
        .into_iter()
        .map(|a| {
            let ver = a.detect();
            AgentInfo {
                id: a.id().to_string(),
                name: a.display_name().to_string(),
                installed: ver.is_some(),
                version: ver,
                supports_hooks: a.supports_hooks(),
            }
        })
        .collect()
}

/// Starts a local-only (`127.0.0.1:0`) authenticated TCP hook listener (Section 4.5 / 4.7).
/// Rejects any message whose `token` does not match `expected_token`.
pub async fn start_hook_listener(
    expected_token: String,
    hook_tx: mpsc::UnboundedSender<HookMessage>,
) -> AppResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        while let Ok((stream, _peer)) = listener.accept().await {
            let expected = expected_token.clone();
            let tx = hook_tx.clone();
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() {
                    if let Ok(msg) = serde_json::from_str::<HookMessage>(line.trim()) {
                        if msg.token == expected {
                            let _ = tx.send(msg);
                        }
                    }
                }
            });
        }
    });

    Ok(port)
}

/// Fast non-GUI helper invoked when the binary is launched as:
/// `beehaven --beehaven-hook <port> <token> <task_id> <event>`
pub fn try_handle_cli_hook_from_args() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(pos) = args.iter().position(|a| a == "--beehaven-hook") else {
        return false;
    };
    if args.len() < pos + 5 {
        return true;
    }
    let Ok(port) = args[pos + 1].parse::<u16>() else {
        return true;
    };
    let token = &args[pos + 2];
    let task_id = &args[pos + 3];
    let event = &args[pos + 4];

    // Read optional JSON line from stdin if non-blocking / piped
    let mut detail = serde_json::Value::Null;
    if !atty_stdin_heuristic() {
        let stdin = std::io::stdin();
        let mut first_line = String::new();
        if BufReader::new(stdin.lock())
            .read_line(&mut first_line)
            .is_ok()
        {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&first_line) {
                detail = parsed;
            }
        }
    }

    let msg = HookMessage {
        token: token.clone(),
        task_id: task_id.clone(),
        event: event.clone(),
        detail,
    };
    if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
        if let Ok(serialized) = serde_json::to_string(&msg) {
            let _ = stream.write_all(format!("{serialized}\n").as_bytes());
            let _ = stream.flush();
        }
    }
    true
}

fn atty_stdin_heuristic() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionCallbackEvent, SessionManager};
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn authenticated_hook_socket_rejects_bad_token_and_signals_waiting_for_you() {
        let (hook_tx, mut hook_rx) = mpsc::unbounded_channel();
        let secret = "test-secret-token-12345".to_string();
        let port = start_hook_listener(secret.clone(), hook_tx).await.unwrap();

        // 1. Forged token must be ignored
        {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            use tokio::io::AsyncWriteExt;
            s.write_all(b"{\"token\":\"wrong\",\"task_id\":\"t1\",\"event\":\"Notification\"}\n")
                .await
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(120), hook_rx.recv())
                .await
                .is_err(),
            "Forged hook token must be rejected"
        );

        // 2. Run fake-agent in PTY with ask-hook mode; verify Notification & Stop events arrive!
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SessionManager::new(tmp.path().join("logs"));
        let (pty_tx, mut pty_rx) = mpsc::unbounded_channel();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("fake-agent.mjs");

        mgr.spawn_session(
            "t-hook".into(),
            "s-hook".into(),
            CommandSpec {
                program: "node".into(),
                args: vec![script.to_string_lossy().into(), "ask-hook".into()],
                env: vec![
                    ("BEEHAVEN_HOOK_PORT".into(), port.to_string()),
                    ("BEEHAVEN_HOOK_TOKEN".into(), secret.clone()),
                    ("BEEHAVEN_TASK_ID".into(), "t-hook".into()),
                ],
                cwd: tmp.path().to_path_buf(),
            },
            80,
            24,
            None,
            pty_tx,
        )
        .unwrap();

        let msg1 = tokio::time::timeout(Duration::from_secs(5), hook_rx.recv())
            .await
            .expect("timed out waiting for Notification hook")
            .unwrap();
        assert_eq!(msg1.task_id, "t-hook");
        assert_eq!(msg1.event, "Notification");
        assert_eq!(
            ClaudeCodeAdapter.interpret(&AgentSignal::HookEvent(msg1.event)),
            Some(TaskState::WaitingForYou)
        );

        // Send user response into PTY stdin so agent finishes and fires Stop hook
        mgr.write_input("t-hook", "yes\r\n").unwrap();

        let msg2 = tokio::time::timeout(Duration::from_secs(5), hook_rx.recv())
            .await
            .expect("timed out waiting for Stop hook")
            .unwrap();
        assert_eq!(msg2.event, "Stop");
        assert_eq!(
            ClaudeCodeAdapter.interpret(&AgentSignal::HookEvent(msg2.event)),
            Some(TaskState::NeedsReview)
        );

        // Drain exit
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(ev) = pty_rx.recv().await {
                if matches!(ev, SessionCallbackEvent::Exited { .. }) {
                    break;
                }
            }
        })
        .await;
    }
}
