import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

// Lightweight xterm mock for jsdom unit tests (real WebGL/canvas tested in browser/Tauri)
vi.mock("@xterm/xterm", () => ({
  Terminal: class MockTerminal {
    cols = 80;
    rows = 24;
    loadAddon() {}
    open() {}
    write() {}
    onData() {
      return { dispose() {} };
    }
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class MockFitAddon {
    fit() {}
  },
}));
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class MockWebglAddon {
    onContextLoss() {}
    dispose() {}
  },
}));

import { App } from "../App";
import { StatusBadge } from "../components/StatusBadge";

describe("Beehaven UI & Accessibility Contracts", () => {
  it("never relies on color alone for status badges (shape + text label + role)", () => {
    render(<StatusBadge state="WaitingForYou" />);
    const badge = screen.getByRole("status", { name: /Waiting for you/i });
    expect(badge).toHaveTextContent("▲");
    expect(badge).toHaveTextContent("Waiting for you");
  });

  it("renders priority-grouped task board with Waiting for you at the top and navigates screens", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.getAllByText(/Add login validation/i).length).toBeGreaterThan(0);
    });

    // Verify global footer metrics
    expect(screen.getByText(/✗ 1 pair/i)).toBeInTheDocument();

    // Open Changes and Tests tabs on Board (regression test for React 19 Zustand snapshot caching!)
    fireEvent.click(screen.getByRole("tab", { name: /^Changes$/i }));
    await waitFor(() => {
      expect(screen.getByText(/changed file\(s\) vs/i)).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("tab", { name: /^Tests$/i }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Run tests in worktree/i })).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: /Run tests in worktree/i }));
    await waitFor(() => {
      expect(screen.getAllByText(/✔ Passed/i).length).toBeGreaterThan(0);
    });

    // Navigate to Conflict Matrix screen
    fireEvent.click(screen.getAllByRole("button", { name: /^Conflicts/i })[0]!);
    await waitFor(() => {
      expect(screen.getByText(/Pairwise Conflict Forecast/i)).toBeInTheDocument();
      expect(screen.getByText(/Suggested Merge Order/i)).toBeInTheDocument();
    });

    // Navigate to Merge Queue screen
    fireEvent.click(screen.getAllByRole("button", { name: /^Merge Queue/i })[0]!);
    await waitFor(() => {
      expect(screen.getByText(/Test-Gated Merge Queue/i)).toBeInTheDocument();
    });

    // Open Command Palette via Ctrl+K
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await waitFor(() => {
      expect(
        screen.getByRole("dialog", { name: /Command palette/i })
      ).toBeInTheDocument();
    });
  });
});
