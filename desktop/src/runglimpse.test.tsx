// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { GONE_MS, RunGlimpse } from "./RunGlimpse";

/**
 * **THE SMALLEST RUN VIEW, FOR A RUN THIS WINDOW NEVER STARTED.** Read from the
 * ledger, not followed live — and the act on a handed step stays `Handed`'s:
 * this view offers no gesture `Handed` does not already offer.
 */

afterEach(cleanup);

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

function pretendShell(answer: (command: string, args?: Record<string, unknown>) => unknown) {
  const calls: Call[] = [];
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        try {
          return Promise.resolve(answer(command, args));
        } catch (error) {
          return Promise.reject(error);
        }
      },
    },
  };
  return {
    calls,
    stop: () => {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    },
  };
}

const REVIEW = {
  step_id: "review",
  holder: "mira",
  mandate: "read the diff and say whether it holds",
  since: 100,
  worktree: "/work/a-tree",
};

describe("the smallest run view", () => {
  test("reads the run from the ledger and offers the same act as any handed step", async () => {
    const shell = pretendShell((command) => {
      if (command === "run_glimpse") return { run_id: "run-9", flow: "una-decisione", worktree: "/work/a-tree" };
      if (command === "run_usage") return null;
      if (command === "handed_steps") return [REVIEW];
      if (command === "close_handed_step") return "step review closed by mira: went\nThe run is resuming.";
      throw new Error(`no ${command}`);
    });
    let closed = 0;
    try {
      render(<RunGlimpse runId="run-9" onClose={() => {}} onChanged={() => (closed += 1)} />);
      await screen.findByText("una-decisione");
      expect(screen.getByText("/work/a-tree")).toBeTruthy();
      // Reached the same act `Handed` always offers, not a copy of it.
      await screen.findByText("read the diff and say whether it holds");

      fireEvent.click(screen.getByRole("button", { name: "close: it went" }));
      await screen.findByText(/The run is resuming/);
      expect(shell.calls.find((call) => call.command === "close_handed_step")?.args).toEqual({
        runId: "run-9",
        stepId: "review",
        outcome: "went",
        said: "",
      });
      await waitFor(() => expect(closed).toBe(1));
    } finally {
      shell.stop();
    }
  });

  test("a run the ledger does not know either says so, instead of showing a blank view", async () => {
    const shell = pretendShell((command) => {
      if (command === "run_glimpse") throw new Error("no run called run-9");
      if (command === "run_usage") return null;
      throw new Error(`no ${command}`);
    });
    try {
      render(<RunGlimpse runId="run-9" onClose={() => {}} />);
      await screen.findByText(/Cannot read this run: Error: no run called run-9/);
    } finally {
      shell.stop();
    }
  });

  test("a run the ledger no longer has closes itself, not staying up forever", async () => {
    vi.useFakeTimers();
    const shell = pretendShell((command) => {
      if (command === "run_glimpse") throw new Error("no run called run-9");
      if (command === "run_usage") return null;
      throw new Error(`no ${command}`);
    });
    let closed = 0;
    try {
      render(<RunGlimpse runId="run-9" onClose={() => (closed += 1)} />);
      await vi.waitFor(() => screen.getByText(/Cannot read this run: Error: no run called run-9/));
      expect(closed).toBe(0);
      await vi.advanceTimersByTimeAsync(GONE_MS);
      expect(closed).toBe(1);
    } finally {
      shell.stop();
      vi.useRealTimers();
    }
  });

  test("closing the view calls back, and asks the ledger nothing further", () => {
    const shell = pretendShell((command) => {
      if (command === "run_glimpse") return { run_id: "run-9", flow: "una-decisione", worktree: null };
      if (command === "run_usage") return null;
      if (command === "handed_steps") return [];
      throw new Error(`no ${command}`);
    });
    let closed = false;
    try {
      render(<RunGlimpse runId="run-9" onClose={() => (closed = true)} />);
      fireEvent.click(screen.getByTitle("close the view"));
      expect(closed).toBe(true);
    } finally {
      shell.stop();
    }
  });
});
