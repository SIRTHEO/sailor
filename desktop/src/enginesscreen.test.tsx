// @vitest-environment jsdom
/**
 * **THE ENGINES SCREEN SHOWS THE EVIDENCE AND OPENS A TERMINAL FOR A GESTURE.**
 * A sign-in runs the descriptor's line there; an install line waits there for
 * Enter; an absent engine offers no sign-in, and a signed-in one none either.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { EnginesScreen, percent } from "./EnginesScreen";
import type { Engines } from "./engines";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const ANSWER: Engines = {
  workspace_root: "/home/theo/personal/sailor",
  engines: [
    {
      id: "codex", label: "Codex", presence: "present", reason: "found `codex` in /opt/homebrew/bin/codex",
      executable: "/opt/homebrew/bin/codex", version: "0.152.1",
      signed_in: "no", signed_in_said: "Not logged in", profile_in_force: "prove",
      quota: [], quota_why: "this engine declares no channel to read what is left",
      sign_in: { program: "/opt/homebrew/bin/codex", args: ["login"], interactive: true, note: "measured with `codex login --help`" },
      install: { line: "npm install -g @openai/codex", note: "measured" },
    },
    {
      id: "claude-code", label: "Claude Code", presence: "present", reason: "found `claude`",
      executable: "/home/theo/.local/bin/claude", version: "2.1.258",
      signed_in: "yes", signed_in_said: "loggedIn: true", profile_in_force: null,
      quota: [{ engine: "claude-code", unit: "seven_day", spent_fraction: 0.42, resets_at: "2026-09-06T04:59:59Z", observed_at: 1 }],
      quota_why: null,
      sign_in: { program: "/home/theo/.local/bin/claude", args: ["auth", "login"], interactive: true, note: "" },
      install: null,
    },
    {
      id: "openrouter-cli", label: "OpenRouter CLI", presence: "absent", reason: "no `openrouter` on the PATH",
      executable: null, version: null,
      signed_in: "not known", signed_in_said: "it is not on this machine, so there is nobody to ask", profile_in_force: null,
      quota: [], quota_why: "this engine declares no channel to read what is left",
      sign_in: null, install: { line: "npm install -g openrouter", note: "" },
    },
  ],
};

function shellThatAnswers(): { calls: Array<{ command: string; args: unknown }> } {
  const calls: Array<{ command: string; args: unknown }> = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string, args: unknown) => {
        calls.push({ command, args });
        if (command === "engines") return Promise.resolve(ANSWER);
        if (command === "terminal_open") return Promise.resolve({ id: "t-1", workspaceRoot: "/home/theo/personal/sailor", alive: true });
        if (command === "terminal_press") return Promise.resolve(undefined);
        return Promise.reject(new Error(`unexpected: ${command}`));
      },
    },
  };
  return { calls };
}

describe("the engines screen", () => {
  test("EVERY LINE CARRIES ITS EVIDENCE, and the window is a percentage spent", async () => {
    shellThatAnswers();
    render(<EnginesScreen native />);
    await waitFor(() => expect(screen.getByText("Codex")).toBeTruthy());
    expect(screen.getByText("found `codex` in /opt/homebrew/bin/codex")).toBeTruthy();
    expect(screen.getByText("Not logged in")).toBeTruthy();
    expect(screen.getByText(/as prove/)).toBeTruthy();
    expect(screen.getByText(/seven_day: 42% spent/)).toBeTruthy();
    expect(screen.getByText("not on this machine")).toBeTruthy();
    expect(percent(0.126)).toBe("13% spent");
  });

  test("A SIGN-IN OPENS A TERMINAL WITH THE DESCRIPTOR'S LINE, and only where it is needed", async () => {
    const shell = shellThatAnswers();
    let shown = 0;
    render(<EnginesScreen native onTerminalOpened={() => { shown += 1; }} />);
    await waitFor(() => expect(screen.getByText("Codex")).toBeTruthy());
    // The controls first: the signed-in engine and the absent one offer no sign-in.
    expect(screen.getAllByText("sign in, in a terminal")).toHaveLength(1);
    fireEvent.click(screen.getByText("sign in, in a terminal"));
    await waitFor(() => expect(shown).toBe(1));
    const opened = shell.calls.find((call) => call.command === "terminal_open");
    expect(opened?.args).toMatchObject({ workspaceRoot: "/home/theo/personal/sailor", program: "/opt/homebrew/bin/codex", args: ["login"] });
  });

  test("AN INSTALL LINE IS TYPED, NOT RUN: the terminal opens bare and the line waits for Enter", async () => {
    const shell = shellThatAnswers();
    render(<EnginesScreen native />);
    await waitFor(() => expect(screen.getByText("OpenRouter CLI")).toBeTruthy());
    fireEvent.click(screen.getByText(/type «npm install -g openrouter»/));
    await waitFor(() => expect(shell.calls.some((call) => call.command === "terminal_press")).toBe(true));
    const opened = shell.calls.find((call) => call.command === "terminal_open");
    expect((opened?.args as { program?: string }).program).toBeUndefined();
    // The bytes cross the bridge as base64, the contract of `pressKeys`.
    const pressed = shell.calls.find((call) => call.command === "terminal_press");
    expect(atob((pressed?.args as { bytes: string }).bytes)).toBe("npm install -g openrouter");
  });

  test("an engine that cannot answer is said, never drawn as a machine without engines", async () => {
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: { invoke: () => Promise.reject(new Error("no engines here")) },
    };
    render(<EnginesScreen native />);
    await waitFor(() => expect(screen.getByText(/I cannot look at the engines: .*no engines here/)).toBeTruthy());
  });
});
