// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { Handed } from "./Handed";

/**
 * **A HANDED STEP IS TAKEN AND CLOSED THROUGH THE ENGINE'S OWN COMMANDS**,
 * and the screen shows what they answered — the refusal included, because
 * the lock on who may judge what is the engine's and not the window's.
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

const REVIEW = { step_id: "review", holder: "theo", mandate: "read the diff and say whether it holds", since: 100 };

describe("a step handed to a person", () => {
  test("shows the mandate, is taken and closed with what was done, and the run reads again", async () => {
    const shell = pretendShell((command) => {
      if (command === "handed_steps") return [REVIEW];
      if (command === "take_handed_step") return "step review opened by theo";
      if (command === "close_handed_step") return "step review closed by theo: went\nThe run is resuming.";
      throw new Error(`no ${command}`);
    });
    let changed = 0;
    try {
      render(<Handed runId="relay-1" onChanged={() => (changed += 1)} />);
      await screen.findByText("read the diff and say whether it holds");
      expect(screen.getByText("offered to «theo»")).toBeTruthy();

      fireEvent.click(screen.getByRole("button", { name: "take it" }));
      await screen.findByText("step review opened by theo");
      expect(shell.calls.find((call) => call.command === "take_handed_step")?.args).toEqual({
        runId: "relay-1",
        stepId: "review",
      });

      fireEvent.change(screen.getByLabelText("what you did for review"), { target: { value: "it holds" } });
      fireEvent.click(screen.getByRole("button", { name: "close: it went" }));
      await screen.findByText(/The run is resuming/);
      expect(shell.calls.find((call) => call.command === "close_handed_step")?.args).toEqual({
        runId: "relay-1",
        stepId: "review",
        outcome: "went",
        said: "it holds",
      });
      await waitFor(() => expect(changed).toBe(2));
    } finally {
      shell.stop();
    }
  });

  test("a refusal is shown as the engine said it, and nothing is marked done", async () => {
    const shell = pretendShell((command) => {
      if (command === "handed_steps") return [REVIEW];
      if (command === "close_handed_step") throw new Error("theo wrote «build», the step this one judges: refused");
      throw new Error(`no ${command}`);
    });
    let changed = 0;
    try {
      render(<Handed runId="relay-1" onChanged={() => (changed += 1)} />);
      await screen.findByText("read the diff and say whether it holds");
      fireEvent.click(screen.getByRole("button", { name: "close: it broke" }));
      await screen.findByText(/the step this one judges: refused/);
      expect(changed).toBe(0);
    } finally {
      shell.stop();
    }
  });

  test("a run that waits on nothing handed says so instead of showing an empty box", async () => {
    const shell = pretendShell((command) => {
      if (command === "handed_steps") return [];
      throw new Error(`no ${command}`);
    });
    try {
      render(<Handed runId="relay-1" />);
      await screen.findByText(/No step of this run is handed to a person/);
    } finally {
      shell.stop();
    }
  });
});
