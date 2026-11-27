use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{AppError, AppResult};
use crate::git::safety::{normalize_existing_path, run_git};
use crate::models::TestRun;
use crate::time_util::now_rfc3339;

/// Executes a repository's configured `test_command` inside `worktree_path`,
/// streaming output chunks to `on_chunk`, enforcing `timeout`, saving full logs to disk,
/// and returning a structured `TestRun` tied to the current `HEAD` commit SHA.
pub async fn run_test_command<F>(
    task_id: &str,
    worktree_path: &Path,
    test_command: &str,
    logs_dir: &Path,
    timeout: Duration,
    mut on_chunk: F,
) -> AppResult<TestRun>
where
    F: FnMut(String) + Send,
{
    let safe_wt = normalize_existing_path(worktree_path)?;
    std::fs::create_dir_all(logs_dir)?;

    let head_out = run_git(&safe_wt, &["rev-parse", "HEAD"])?;
    let commit_sha = String::from_utf8_lossy(&head_out.stdout).trim().to_string();

    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = now_rfc3339();
    let timer = Instant::now();
    let log_path = logs_dir.join(format!("test-{task_id}-{run_id}.log"));

    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoLogo", "-NoProfile", "-Command", test_command]);
        c.creation_flags(0x0800_0000);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", test_command]);
        c
    };

    cmd.current_dir(&safe_wt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd.spawn().map_err(|e| {
        AppError::with_details(
            "TEST_SPAWN_FAILED",
            format!("Couldn't start test command '{test_command}'."),
            e.to_string(),
        )
    })?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let mut out_lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();

    let mut full_log = String::new();
    let mut out_done = false;
    let mut err_done = false;
    let mut timed_out = false;

    let sleep = tokio::time::sleep(timeout);
    tokio::pin!(sleep);

    while !(out_done && err_done) {
        tokio::select! {
            _ = &mut sleep => {
                timed_out = true;
                let _ = child.kill().await;
                let msg = format!("\n[Beehaven] Test command timed out after {}s.\n", timeout.as_secs());
                full_log.push_str(&msg);
                on_chunk(msg);
                break;
            }
            line_res = out_lines.next_line(), if !out_done => {
                match line_res {
                    Ok(Some(line)) => {
                        let formatted = format!("{line}\n");
                        full_log.push_str(&formatted);
                        on_chunk(formatted);
                    }
                    _ => out_done = true,
                }
            }
            line_res = err_lines.next_line(), if !err_done => {
                match line_res {
                    Ok(Some(line)) => {
                        let formatted = format!("{line}\n");
                        full_log.push_str(&formatted);
                        on_chunk(formatted);
                    }
                    _ => err_done = true,
                }
            }
        }
    }

    let status = child.wait().await.ok();
    let passed = !timed_out && status.map(|s| s.success()).unwrap_or(false);
    let duration_ms = timer.elapsed().as_millis() as i64;

    std::fs::write(&log_path, &full_log)?;

    // Keep last 16 KB for instant UI preview
    let preview = if full_log.len() > 16_384 {
        let mut idx = full_log.len() - 16_384;
        while !full_log.is_char_boundary(idx) {
            idx += 1;
        }
        full_log[idx..].to_string()
    } else {
        full_log
    };

    Ok(TestRun {
        id: run_id,
        task_id: task_id.to_string(),
        commit_sha,
        passed,
        log_path: log_path.to_string_lossy().to_string(),
        output_preview: preview,
        started_at,
        duration_ms,
    })
}
