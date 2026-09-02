// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { ChangesScreen } from "./ChangesScreen";
import { statusWord } from "./changes";

/**
 * **WHAT IS SHOWN IS GIT'S ANSWER, VERBATIM.** The screen draws the files and
 * the diff the engine handed it and computes nothing of its own: the test
 * feeds a diff no code would produce and expects it back unchanged. And a
 * file goes to the editor by its absolute path, or the editor opens nothing.
 */

afterEach(cleanup);

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

function pretendShell(answers: Record<string, unknown>): { calls: Call[]; stop: () => void } {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  const calls: Call[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        if (!(command in answers)) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(answers[command]);
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

const SEEN = {
  root: "/work/sailor",
  files: [
    { path: "crates/terminal/src/host.rs", status: " M" },
    { path: "docs/new-note.md", status: "??" },
  ],
  diff: "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a line git alone would write\n+and this one too\n",
};

describe("what changed in a workspace", () => {
  test("THE FILES AND THE DIFF ARE THE ENGINE'S, and the diff is shown as it came", async () => {
    const shell = pretendShell({ workspace_changes: SEEN, open_in_editor: null });
    try {
      render(<ChangesScreen root="/work/sailor" name="sailor" />);
      await screen.findByText("crates/terminal/src/host.rs");
      expect(shell.calls.map((call) => call.command)).toContain("workspace_changes");
      expect(shell.calls.find((call) => call.command === "workspace_changes")?.args).toEqual({ root: "/work/sailor" });
      expect(screen.getByText("docs/new-note.md")).toBeTruthy();
      expect(screen.getByText("new")).toBeTruthy();
      expect(screen.getByText("changed")).toBeTruthy();
      expect(document.querySelector(".changes__diff")?.textContent).toBe(SEEN.diff);

      // A FILE GOES TO THE EDITOR BY ITS ABSOLUTE PATH.
      await act(async () => {
        fireEvent.click(screen.getAllByRole("button", { name: "open in the editor" })[1]);
      });
      expect(shell.calls.find((call) => call.command === "open_in_editor")?.args).toEqual({
        path: "/work/sailor/docs/new-note.md",
      });
    } finally {
      shell.stop();
    }
  });

  test("an engine that cannot read the tree is said, not passed for a clean tree", async () => {
    const shell = pretendShell({});
    try {
      render(<ChangesScreen root="/work/sailor" name="sailor" />);
      await screen.findByText(/I cannot read the working tree/);
      expect(screen.queryByText(/Nothing changed/)).toBeNull();
    } finally {
      shell.stop();
    }
  });

  test("a porcelain status has a word", () => {
    expect(statusWord("??")).toBe("new");
    expect(statusWord(" M")).toBe("changed");
    expect(statusWord("A ")).toBe("added");
    expect(statusWord(" D")).toBe("deleted");
  });
});
