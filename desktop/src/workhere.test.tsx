// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { Projects } from "./Projects";

/**
 * **MOVING INTO A PROJECT IS ASKED OF THE ENGINE, AND THE LIST IS READ AGAIN
 * FROM IT.** The row the window stands in offers no move; a refused move is
 * written with the engine's reason instead of marking the row «here» by hand.
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

const HERE = { root: "/work/sailor", name: "sailor", first_seen: 1, last_seen: 2, standing: "declared", current: true };
const THERE = { root: "/work/orca", name: "orca", first_seen: 1, last_seen: 2, standing: "declared", current: false };
const GONE = { root: "/work/lost", name: "lost", first_seen: 1, last_seen: 2, standing: "gone", current: false };

describe("moving the window into a project", () => {
  test("only the other, still-declared projects offer the move, and it goes to the engine", async () => {
    let moved = false;
    const shell = pretendShell((command, args) => {
      if (command === "workspaces") {
        return moved
          ? [{ ...HERE, current: false }, { ...THERE, current: true }, GONE]
          : [HERE, THERE, GONE];
      }
      if (command === "work_here") {
        moved = true;
        return args?.root;
      }
      throw new Error(`no ${command}`);
    });
    let told = 0;
    try {
      render(<Projects native now={10} onMoved={() => (told += 1)} />);
      const buttons = await screen.findAllByRole("button", { name: "work here" });
      expect(buttons).toHaveLength(1);

      fireEvent.click(buttons[0]);
      await waitFor(() => expect(told).toBe(1));
      const asked = shell.calls.find((call) => call.command === "work_here");
      expect(asked?.args).toEqual({ root: "/work/orca" });

      // The list is the engine's answer, read again: «here» moved with it.
      await waitFor(() => {
        const here = document.querySelector("tr[data-here]");
        expect(here?.textContent).toContain("orca");
      });
    } finally {
      shell.stop();
    }
  });

  test("a refused move is written with the engine's reason, and nothing moves", async () => {
    const shell = pretendShell((command) => {
      if (command === "workspaces") return [HERE, THERE];
      if (command === "work_here") throw new Error("no sailor.json at or above /work/orca: not a project");
      throw new Error(`no ${command}`);
    });
    let told = 0;
    try {
      render(<Projects native now={10} onMoved={() => (told += 1)} />);
      fireEvent.click(await screen.findByRole("button", { name: "work here" }));
      await screen.findByText(/The move was refused: .*not a project/);
      expect(told).toBe(0);
      expect(document.querySelector("tr[data-here]")?.textContent).toContain("sailor");
    } finally {
      shell.stop();
    }
  });
});
