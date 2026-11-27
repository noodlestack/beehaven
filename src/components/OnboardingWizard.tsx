import { useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, isTauriRuntime } from "../lib/ipc";
import { useAppStore } from "../store/useAppStore";
import type { RepoCheckResult } from "../types/models";

export function OnboardingWizard() {
  const open = useAppStore((s) => s.onboardingOpen);
  const setOpen = useAppStore((s) => s.setOnboardingOpen);
  const openRepo = useAppStore((s) => s.openRepo);
  const createDemoRepo = useAppStore((s) => s.createDemoRepo);
  const settings = useAppStore((s) => s.settings);
  const updateSettings = useAppStore((s) => s.updateSettings);
  const upsertTaskLocal = useAppStore((s) => s.upsertTaskLocal);
  const selectTask = useAppStore((s) => s.selectTask);
  const setError = useAppStore((s) => s.setError);
  const showToast = useAppStore((s) => s.showToast);

  const [repoPath, setRepoPath] = useState("");
  const [check, setCheck] = useState<RepoCheckResult | null>(null);
  const [baseBranch, setBaseBranch] = useState("main");
  const [testCommand, setTestCommand] = useState("cargo test");
  const [chosenAgent, setChosenAgent] = useState(settings.default_agent);
  const [sampleTitle, setSampleTitle] = useState("");
  const [samplePrompt, setSamplePrompt] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const inspectFolder = async (folder: string) => {
    setRepoPath(folder);
    if (!folder.trim()) return;
    try {
      const res = await api.repoCheck(folder.trim());
      setCheck(res);
      setBaseBranch(res.detected_base_branch);
      setTestCommand(res.suggested_test_command);
      const installed = res.detected_agents.find((a) => a.installed);
      if (installed) setChosenAgent(installed.id);
    } catch (err) {
      setError(err);
    }
  };

  const handleBrowse = async () => {
    if (!isTauriRuntime()) {
      await inspectFolder("C:/projects/sample-repo");
      return;
    }
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string" && picked) {
        await inspectFolder(picked);
      }
    } catch (err) {
      setError(err);
    }
  };

  const handleFinish = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!repoPath.trim()) return;
    setBusy(true);
    try {
      const repo = await openRepo(
        repoPath.trim(),
        baseBranch.trim(),
        testCommand.trim()
      );
      if (!repo) return;

      await updateSettings({
        ...settings,
        default_agent: chosenAgent,
        onboarding_completed: true,
      });

      if (sampleTitle.trim()) {
        const created = await api.taskCreate({
          repoId: repo.id,
          title: sampleTitle.trim(),
          prompt: samplePrompt.trim() || sampleTitle.trim(),
          agent: chosenAgent,
          autoStart: true,
        });
        upsertTaskLocal(created);
        selectTask(created.id);
      }

      setOpen(false);
      showToast(`Opened repository "${repo.name}". Ready to run parallel tasks.`);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="First-run repository setup"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 p-4"
    >
      <form
        onSubmit={(e) => void handleFinish(e)}
        style={{
          backgroundColor: "var(--af-surface)",
          borderColor: "var(--af-border)",
          borderRadius: "var(--af-radius-panel)",
        }}
        className="flex max-h-[92vh] w-full max-w-xl flex-col gap-4 overflow-y-auto border p-6 shadow-2xl"
      >
        <div className="flex items-start justify-between gap-2">
          <div>
            <h2 className="m-0 text-xl font-semibold">Set up Beehaven</h2>
            <p className="m-0 mt-1 text-xs" style={{ color: "var(--af-muted)" }}>
              Connect a local Git repository, or launch a safe 1-click practice sandbox to test-drive every feature first.
            </p>
          </div>
          <button
            type="button"
            className="af-btn py-1"
            onClick={() => setOpen(false)}
          >
            Close
          </button>
        </div>

        {/* Instant Practice Sandbox Callout */}
        <div
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderColor: "var(--af-success)",
            borderRadius: "var(--af-radius-control)",
          }}
          className="flex flex-wrap items-center justify-between gap-3 border p-3 text-xs"
        >
          <div>
            <strong style={{ color: "var(--af-success)" }}>
              New to Beehaven? Try it safely in 1 click
            </strong>
            <div style={{ color: "var(--af-muted)" }}>
              Creates a real sample Git repository with 4 worktrees, passing tests, and a live merge conflict demo.
            </div>
          </div>
          <button
            type="button"
            disabled={busy}
            className="af-btn af-btn-success shrink-0"
            onClick={() => {
              setBusy(true);
              void createDemoRepo()
                .then((r) => {
                  if (r) {
                    void updateSettings({ ...settings, onboarding_completed: true });
                    showToast("Practice sandbox ready! Check Board, Conflicts, and Merge Queue.");
                  }
                })
                .finally(() => setBusy(false));
            }}
          >
            ✓ Create Practice Sandbox
          </button>
        </div>

        {/* Step 1: Repository */}
        <div className="flex flex-col gap-1.5 text-xs">
          <label htmlFor="onboard-repo-path" className="font-semibold">
            1. Choose a local Git repository
          </label>
          <div className="flex gap-2">
            <input
              id="onboard-repo-path"
              required
              type="text"
              className="af-input"
              placeholder="C:/path/to/your/git-repo"
              value={repoPath}
              onChange={(e) => setRepoPath(e.target.value)}
              onBlur={() => void inspectFolder(repoPath)}
            />
            <button
              type="button"
              className="af-btn shrink-0"
              onClick={() => void handleBrowse()}
            >
              Browse…
            </button>
            <button
              type="button"
              className="af-btn shrink-0"
              onClick={() => void inspectFolder(repoPath)}
            >
              Check
            </button>
          </div>

          {check && (
            <div
              style={{
                backgroundColor: "var(--af-surface-elevated)",
                borderColor:
                  check.git_version_ok && check.is_git_repo
                    ? "var(--af-success)"
                    : "var(--af-danger)",
                borderRadius: "var(--af-radius-control)",
              }}
              className="mt-1 border p-2.5 text-xs"
            >
              <div>
                {check.git_version_ok ? "✓" : "✗"} {check.git_version} (requires Git 2.38+)
              </div>
              <div>
                {check.is_git_repo
                  ? `✓ Valid Git repository (base: ${check.detected_base_branch})`
                  : "✗ Folder is not a Git repository"}
              </div>
            </div>
          )}
        </div>

        {/* Step 2: Base branch & Test command */}
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">2. Base branch</span>
            <input
              type="text"
              className="af-input"
              value={baseBranch}
              onChange={(e) => setBaseBranch(e.target.value)}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            <span className="font-semibold">3. Verification test command</span>
            <input
              type="text"
              className="af-input af-mono"
              value={testCommand}
              onChange={(e) => setTestCommand(e.target.value)}
            />
          </label>
        </div>

        {/* Step 3: Agent CLI & Permissions notice (Section 4.7) */}
        <div className="flex flex-col gap-1 text-xs">
          <label htmlFor="onboard-agent" className="font-semibold">
            4. Default coding agent CLI
          </label>
          <select
            id="onboard-agent"
            className="af-input"
            value={chosenAgent}
            onChange={(e) => setChosenAgent(e.target.value)}
          >
            {(check?.detected_agents ?? [
              { id: "claude", name: "Claude Code", installed: true, version: "detected", supports_hooks: true },
              { id: "generic", name: "Interactive Shell / Custom CLI", installed: true, version: "built-in", supports_hooks: false },
            ]).map((a) => (
              <option key={a.id} value={a.id}>
                {a.name} — {a.installed ? a.version ?? "installed" : "not detected"}
              </option>
            ))}
          </select>
          <p className="m-0 mt-1" style={{ color: "var(--af-muted)" }}>
            Security note: Coding agents run locally with your OS user account’s permissions inside sibling worktree folders (<code className="af-mono">&lt;repo&gt;.worktrees/</code>).
          </p>
        </div>

        {/* Step 4: Optional First Sample Task */}
        <div
          style={{
            backgroundColor: "var(--af-surface-elevated)",
            borderColor: "var(--af-border)",
            borderRadius: "var(--af-radius-control)",
          }}
          className="flex flex-col gap-2 border p-3 text-xs"
        >
          <div className="flex items-center justify-between">
            <span className="font-semibold">Optional: Add your first task now</span>
            <button
              type="button"
              className="af-btn py-0.5 text-[11px]"
              onClick={() => {
                setSampleTitle("Audit test coverage & document gaps");
                setSamplePrompt(
                  "Inspect the test suite, add one high-value missing unit test, and verify the test command passes."
                );
              }}
            >
              Fill sample prompt
            </button>
          </div>
          <input
            type="text"
            className="af-input"
            placeholder="First task title (leave blank to skip)"
            value={sampleTitle}
            onChange={(e) => setSampleTitle(e.target.value)}
          />
          {sampleTitle && (
            <textarea
              rows={2}
              className="af-input"
              placeholder="Prompt instructions…"
              value={samplePrompt}
              onChange={(e) => setSamplePrompt(e.target.value)}
            />
          )}
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            className="af-btn"
            onClick={() => setOpen(false)}
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={busy || !repoPath.trim()}
            className="af-btn af-btn-primary"
          >
            {busy ? "Opening…" : "Open repository & finish setup"}
          </button>
        </div>
      </form>
    </div>
  );
}
