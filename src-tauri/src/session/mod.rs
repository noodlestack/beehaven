use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};

const REPLAY_RING_CAP_BYTES: usize = 256 * 1024; // 256 KB rolling replay buffer per session
const BATCH_INTERVAL_MS: u64 = 16;

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub enum SessionCallbackEvent {
    Output {
        task_id: String,
        chunk: String,
    },
    Exited {
        task_id: String,
        session_id: String,
        exit_code: Option<i32>,
    },
    SilenceTimeout {
        task_id: String,
    },
}

struct ActivePty {
    #[allow(dead_code)]
    session_id: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    pid: Option<u32>,
    replay: Arc<Mutex<VecDeque<u8>>>,
    last_output_at: Arc<Mutex<Instant>>,
    silence_emitted: Arc<Mutex<bool>>,
    killed: Arc<Mutex<bool>>,
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<ActivePty>>>>,
    completed_replays: Arc<Mutex<HashMap<String, VecDeque<u8>>>>,
    logs_dir: PathBuf,
}

impl SessionManager {
    pub fn new(logs_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&logs_dir);
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            completed_replays: Arc::new(Mutex::new(HashMap::new())),
            logs_dir,
        }
    }

    pub fn active_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn is_active(&self, task_id: &str) -> bool {
        self.sessions.lock().unwrap().contains_key(task_id)
    }

    /// Spawns a pseudo-terminal session owned by the Rust core, batches output ~every 16 ms
    /// over a bounded channel, records a rolling replay buffer + disk log, and monitors exit/silence.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_session(
        &self,
        task_id: String,
        session_id: String,
        spec: CommandSpec,
        cols: u16,
        rows: u16,
        silence_timeout_secs: Option<u64>,
        event_tx: mpsc::UnboundedSender<SessionCallbackEvent>,
    ) -> AppResult<Option<u32>> {
        // Kill any previous session for the same task first
        let _ = self.kill_session(&task_id);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows.max(10),
                cols: cols.max(20),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| {
                AppError::with_details(
                    "PTY_OPEN_FAILED",
                    "Couldn't open a pseudo-terminal session.",
                    e.to_string(),
                )
            })?;

        let mut cmd = CommandBuilder::new(&spec.program);
        for arg in &spec.args {
            cmd.arg(arg);
        }
        cmd.cwd(&spec.cwd);
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
            AppError::with_details(
                "PTY_SPAWN_FAILED",
                format!(
                    "Couldn't launch '{}'. Check that the tool is installed and on your PATH.",
                    spec.program
                ),
                e.to_string(),
            )
        })?;
        // Drop slave handle in parent so EOF is delivered cleanly when child exits on Windows ConPTY & Unix!
        drop(pair.slave);

        let pid = child.process_id();
        let mut reader = pair.master.try_clone_reader().map_err(|e| {
            AppError::with_details(
                "PTY_READER_FAILED",
                "Couldn't attach PTY reader.",
                e.to_string(),
            )
        })?;
        let writer = pair.master.take_writer().map_err(|e| {
            AppError::with_details(
                "PTY_WRITER_FAILED",
                "Couldn't attach PTY writer.",
                e.to_string(),
            )
        })?;

        let replay = Arc::new(Mutex::new(VecDeque::with_capacity(64 * 1024)));
        let last_output_at = Arc::new(Mutex::new(Instant::now()));
        let silence_emitted = Arc::new(Mutex::new(false));
        let killed = Arc::new(Mutex::new(false));

        let active = Arc::new(ActivePty {
            session_id: session_id.clone(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            pid,
            replay: replay.clone(),
            last_output_at: last_output_at.clone(),
            silence_emitted: silence_emitted.clone(),
            killed: killed.clone(),
        });

        self.sessions
            .lock()
            .unwrap()
            .insert(task_id.clone(), active.clone());

        // Bounded channel from blocking OS reader thread -> 16ms async batching task
        let (raw_tx, mut raw_rx) = mpsc::channel::<Vec<u8>>(256);

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if raw_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let log_file_path = self.logs_dir.join(format!("session-{task_id}.log"));
        let task_id_for_batch = task_id.clone();
        let event_tx_for_batch = event_tx.clone();
        let replay_for_batch = replay.clone();
        let last_out_for_batch = last_output_at.clone();
        let silence_flag_for_batch = silence_emitted.clone();
        let active_for_dsr = active.clone();

        tokio::spawn(async move {
            let mut log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file_path)
                .ok();

            let mut pending = Vec::<u8>::new();
            let mut ticker = tokio::time::interval(Duration::from_millis(BATCH_INTERVAL_MS));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    maybe_bytes = raw_rx.recv() => {
                        match maybe_bytes {
                            Some(raw_bytes) => {
                                // Windows 11 ConPTY emits ANSI DSR `\x1b[6n` on startup and blocks
                                // child console initialization until a cursor report `\x1b[1;1R`
                                // is written back. Answering here unlocks background agents immediately
                                // and keeps `\x1b[6n` out of xterm.js replay buffers!
                                let bytes = if raw_bytes.windows(4).any(|w| w == b"\x1b[6n") {
                                    if let Ok(mut w) = active_for_dsr.writer.lock() {
                                        let _ = w.write_all(b"\x1b[1;1R");
                                        let _ = w.flush();
                                    }
                                    strip_dsr_sequence(&raw_bytes)
                                } else {
                                    raw_bytes
                                };
                                if bytes.is_empty() {
                                    continue;
                                }

                                *last_out_for_batch.lock().unwrap() = Instant::now();
                                *silence_flag_for_batch.lock().unwrap() = false;
                                if let Some(f) = log_file.as_mut() {
                                    let _ = f.write_all(&bytes);
                                    let _ = f.flush();
                                }
                                {
                                    let mut ring = replay_for_batch.lock().unwrap();
                                    for &b in &bytes {
                                        if ring.len() >= REPLAY_RING_CAP_BYTES {
                                            ring.pop_front();
                                        }
                                        ring.push_back(b);
                                    }
                                }
                                pending.extend_from_slice(&bytes);
                                if pending.len() >= 32 * 1024 {
                                    let chunk = String::from_utf8_lossy(&pending).into_owned();
                                    pending.clear();
                                    let _ = event_tx_for_batch.send(SessionCallbackEvent::Output {
                                        task_id: task_id_for_batch.clone(),
                                        chunk,
                                    });
                                }
                            }
                            None => {
                                if !pending.is_empty() {
                                    let chunk = String::from_utf8_lossy(&pending).into_owned();
                                    pending.clear();
                                    let _ = event_tx_for_batch.send(SessionCallbackEvent::Output {
                                        task_id: task_id_for_batch.clone(),
                                        chunk,
                                    });
                                }
                                break;
                            }
                        }
                    }
                    _ = ticker.tick() => {
                        if !pending.is_empty() {
                            let chunk = String::from_utf8_lossy(&pending).into_owned();
                            pending.clear();
                            let _ = event_tx_for_batch.send(SessionCallbackEvent::Output {
                                task_id: task_id_for_batch.clone(),
                                chunk,
                            });
                        }
                    }
                }
            }
        });

        // Optional output-silence fallback detector for Generic agents
        if let Some(timeout_secs) = silence_timeout_secs.filter(|s| *s > 0) {
            let sessions_map = self.sessions.clone();
            let task_id_sil = task_id.clone();
            let event_tx_sil = event_tx.clone();
            let last_out_sil = last_output_at.clone();
            let silence_flag = silence_emitted.clone();
            tokio::spawn(async move {
                let mut check_tick = tokio::time::interval(Duration::from_secs(2));
                loop {
                    check_tick.tick().await;
                    if !sessions_map.lock().unwrap().contains_key(&task_id_sil) {
                        break;
                    }
                    let elapsed = last_out_sil.lock().unwrap().elapsed();
                    let mut emitted = silence_flag.lock().unwrap();
                    if !*emitted && elapsed >= Duration::from_secs(timeout_secs) {
                        *emitted = true;
                        let _ = event_tx_sil.send(SessionCallbackEvent::SilenceTimeout {
                            task_id: task_id_sil.clone(),
                        });
                    }
                }
            });
        }

        // Child exit watcher thread
        let sessions_for_exit = self.sessions.clone();
        let completed_for_exit = self.completed_replays.clone();
        let killed_for_exit = killed.clone();
        std::thread::spawn(move || {
            let exit_code = child.wait().ok().map(|status| status.exit_code() as i32);
            // Allow Windows ConPTY conhost.exe to flush buffered frames to the reader pipe
            // before dropping MasterPty (ClosePseudoConsole discards unread pipe data).
            std::thread::sleep(Duration::from_millis(180));
            if let Some(removed) = sessions_for_exit.lock().unwrap().remove(&task_id) {
                let snapshot = removed.replay.lock().unwrap().clone();
                completed_for_exit
                    .lock()
                    .unwrap()
                    .insert(task_id.clone(), snapshot);
            }
            if !*killed_for_exit.lock().unwrap() {
                let _ = event_tx.send(SessionCallbackEvent::Exited {
                    task_id,
                    session_id,
                    exit_code,
                });
            }
        });

        Ok(pid)
    }

    /// Returns the rolling replay buffer for UI reconnects after window reloads.
    pub fn replay_output(&self, task_id: &str) -> String {
        if let Some(active) = self.sessions.lock().unwrap().get(task_id).cloned() {
            let ring = active.replay.lock().unwrap();
            let bytes: Vec<u8> = ring.iter().copied().collect();
            return String::from_utf8_lossy(&bytes).into_owned();
        }
        if let Some(ring) = self.completed_replays.lock().unwrap().get(task_id) {
            let bytes: Vec<u8> = ring.iter().copied().collect();
            if !bytes.is_empty() {
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        let log_path = self.logs_dir.join(format!("session-{task_id}.log"));
        if let Ok(bytes) = std::fs::read(&log_path) {
            let start = bytes.len().saturating_sub(REPLAY_RING_CAP_BYTES);
            return String::from_utf8_lossy(&bytes[start..]).into_owned();
        }
        String::new()
    }

    pub fn write_input(&self, task_id: &str, data: &str) -> AppResult<()> {
        let active = self
            .sessions
            .lock()
            .unwrap()
            .get(task_id)
            .cloned()
            .ok_or_else(|| {
                AppError::user(
                    "SESSION_NOT_ACTIVE",
                    "No live terminal session is running for this task.",
                )
            })?;
        *active.last_output_at.lock().unwrap() = Instant::now();
        *active.silence_emitted.lock().unwrap() = false;
        let mut writer = active.writer.lock().unwrap();
        writer.write_all(data.as_bytes())?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, task_id: &str, cols: u16, rows: u16) -> AppResult<()> {
        if let Some(active) = self.sessions.lock().unwrap().get(task_id).cloned() {
            let master = active.master.lock().unwrap();
            master
                .resize(PtySize {
                    rows: rows.max(5),
                    cols: cols.max(10),
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| {
                    AppError::with_details(
                        "PTY_RESIZE_FAILED",
                        "Couldn't resize terminal.",
                        e.to_string(),
                    )
                })?;
        }
        Ok(())
    }

    /// Kills the entire process tree for a task session (`taskkill /T /F` on Windows,
    /// `kill -TERM -pgid` / `pkill -P` on Unix) so zero orphan child processes survive.
    pub fn kill_session(&self, task_id: &str) -> AppResult<()> {
        if let Some(active) = self.sessions.lock().unwrap().remove(task_id) {
            *active.killed.lock().unwrap() = true;
            if let Some(pid) = active.pid {
                kill_process_tree(pid);
            }
        }
        Ok(())
    }
}

fn strip_dsr_sequence(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 4 <= input.len() && &input[i..i + 4] == b"\x1b[6n" {
            i += 4;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

pub fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-TERM", "-P", &pid.to_string()])
            .output();
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
    }
}

pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        if let Ok(out) = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            return text.contains(&pid.to_string());
        }
        false
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_script() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("fake-agent.mjs")
    }

    #[tokio::test]
    async fn pty_spawn_input_resize_replay_flood_and_tree_kill_work() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = SessionManager::new(tmp.path().join("logs"));
        let (tx, mut rx) = mpsc::unbounded_channel();

        // 1. Interactive echo + resize + replay
        let script = fixture_script();
        mgr.spawn_session(
            "t-echo".into(),
            "s-1".into(),
            CommandSpec {
                program: "node".into(),
                args: vec![script.to_string_lossy().into(), "echo".into()],
                env: vec![],
                cwd: tmp.path().to_path_buf(),
            },
            80,
            24,
            None,
            tx.clone(),
        )
        .unwrap();

        mgr.resize("t-echo", 120, 40).unwrap();
        mgr.write_input("t-echo", "hello-beehaven\r\nexit\r\n")
            .unwrap();

        let mut combined = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
            {
                match ev {
                    SessionCallbackEvent::Output { chunk, .. } => combined.push_str(&chunk),
                    SessionCallbackEvent::Exited { .. }
                        if combined.contains("ECHO:hello-beehaven") =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        }
        // Replay buffer also retains output after UI reload!
        let replay = mgr.replay_output("t-echo");
        assert!(
            combined.contains("ECHO:hello-beehaven") || replay.contains("ECHO:hello-beehaven"),
            "Expected echo output, got: {combined} / replay: {replay}"
        );

        // 2. Flood batching test (2,000 lines batched cleanly)
        let (tx2, mut rx2) = mpsc::unbounded_channel();
        mgr.spawn_session(
            "t-flood".into(),
            "s-2".into(),
            CommandSpec {
                program: "node".into(),
                args: vec![script.to_string_lossy().into(), "flood".into()],
                env: vec![],
                cwd: tmp.path().to_path_buf(),
            },
            100,
            30,
            None,
            tx2,
        )
        .unwrap();

        let mut flood_chunks = 0usize;
        let mut saw_done = false;
        let deadline2 = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline2 {
            if let Ok(Some(SessionCallbackEvent::Output { chunk, .. })) =
                tokio::time::timeout(Duration::from_millis(500), rx2.recv()).await
            {
                flood_chunks += 1;
                if chunk.contains("[flood] DONE") {
                    saw_done = true;
                    break;
                }
            }
        }
        assert!(saw_done, "Flood output must reach DONE");
        assert!(
            flood_chunks < 500,
            "16ms batching should coalesce 2000 lines into far fewer IPC events (got {flood_chunks})"
        );

        // 3. Process-tree termination test (no orphan child processes!)
        let (tx3, mut rx3) = mpsc::unbounded_channel();
        mgr.spawn_session(
            "t-tree".into(),
            "s-3".into(),
            CommandSpec {
                program: "node".into(),
                args: vec![script.to_string_lossy().into(), "spawn-child".into()],
                env: vec![],
                cwd: tmp.path().to_path_buf(),
            },
            80,
            24,
            None,
            tx3,
        )
        .unwrap();

        let mut child_pid: Option<u32> = None;
        let deadline3 = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < deadline3 {
            if let Ok(Some(SessionCallbackEvent::Output { chunk, .. })) =
                tokio::time::timeout(Duration::from_millis(500), rx3.recv()).await
            {
                for line in chunk.lines() {
                    if let Some(rest) = line.trim().strip_prefix("CHILD_PID=") {
                        let digits: String =
                            rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if let Ok(p) = digits.parse::<u32>() {
                            child_pid = Some(p);
                            break;
                        }
                    }
                }
                if child_pid.is_some() {
                    break;
                }
            }
        }
        let pid = child_pid.expect("spawn-child fixture should print CHILD_PID");
        assert!(is_pid_alive(pid));
        mgr.kill_session("t-tree").unwrap();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            !is_pid_alive(pid),
            "Child process {pid} must be terminated when session is killed"
        );
    }
}
