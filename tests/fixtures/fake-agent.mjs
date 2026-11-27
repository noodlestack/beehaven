#!/usr/bin/env node
/**
 * Cross-platform fake coding agent fixture for Beehaven integration tests.
 *
 * Modes (via first CLI arg or BEEHAVEN_FAKE_MODE env var):
 * - "echo" (default): prints greeting, echoes stdin lines until "exit" or EOF.
 * - "flood": emits 2,000 numbered lines rapidly to test 16ms PTY batching & backpressure.
 * - "ask-hook": sends a Notification event to BEEHAVEN_HOOK_PORT with BEEHAVEN_HOOK_TOKEN,
 *               waits for a stdin reply line, then sends Stop event and exits 0.
 * - "spawn-child": spawns a long-lived child node process and prints its PID so tests can
 *                  verify full process-tree termination when the session is killed.
 */
import net from "node:net";
import readline from "node:readline";
import { spawn } from "node:child_process";

const mode = process.argv[2] || process.env.BEEHAVEN_FAKE_MODE || "echo";

function sendHook(event, detail = {}) {
  return new Promise((resolve, reject) => {
    const port = Number(process.env.BEEHAVEN_HOOK_PORT || 0);
    const token = process.env.BEEHAVEN_HOOK_TOKEN || "";
    const taskId = process.env.BEEHAVEN_TASK_ID || "spike-task";
    if (!port || !token) {
      resolve(false);
      return;
    }
    const payload = JSON.stringify({ token, task_id: taskId, event, detail }) + "\n";
    const client = net.createConnection({ host: "127.0.0.1", port }, () => {
      client.end(payload);
    });
    client.on("close", () => resolve(true));
    client.on("error", (err) => reject(err));
  });
}

async function main() {
  if (mode === "flood") {
    for (let i = 1; i <= 2000; i++) {
      process.stdout.write(`[flood] line ${i.toString().padStart(4, "0")} - agent output payload\n`);
    }
    process.stdout.write("[flood] DONE\n");
    process.exit(0);
  }

  if (mode === "ask-hook") {
    process.stdout.write("[agent] Working on task...\n");
    await sendHook("Notification", { message: "Approve editing src/auth.rs?" });
    process.stdout.write("[agent] Waiting for user input...\n");
    const rl = readline.createInterface({ input: process.stdin, terminal: false });
    for await (const line of rl) {
      process.stdout.write(`[agent] Received answer: ${line.trim()}\n`);
      break;
    }
    await sendHook("Stop", { reason: "completed" });
    process.stdout.write("[agent] Finished cleanly.\n");
    process.exit(0);
  }

  if (mode === "spawn-child") {
    const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
      stdio: "ignore",
    });
    process.stdout.write(`CHILD_PID=${child.pid}\n`);
    setInterval(() => {}, 1000);
    return;
  }

  process.stdout.write("FAKE_AGENT_READY\n");
  const rl = readline.createInterface({ input: process.stdin, terminal: false });
  for await (const line of rl) {
    const trimmed = line.trim();
    if (trimmed === "exit") {
      process.stdout.write("FAKE_AGENT_BYE\n");
      process.exit(0);
    }
    process.stdout.write(`ECHO:${trimmed}\n`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
