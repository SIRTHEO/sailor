// @vitest-environment jsdom
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { TerminalSummary } from "../terminal";
import { terminalGroups } from "./terminalgroups";
import { TheWindow } from "./TheWindow";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

// xterm asks the browser for what jsdom does not have: scaffolding, not a
// fake over the code under test.
beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (window as unknown as { matchMedia: unknown }).matchMedia = () => ({
    matches: false,
    media: "",
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  });
});

function terminal(id: string, root: string, program: string, alive = true): TerminalSummary {
  return {
    id,
    workspaceRoot: root,
    workspaceName: root.split("/").pop() ?? root,
    alive,
    processId: 1,
    device: `ttys00${id}`,
    moved: 0,
    estimatedTokens: 0,
    program,
    profile: null,
  };
}

/** Two in one tree, one in another: the shape a day of work has. */
const THREE = [
  terminal("1", "/somewhere/a-code-project", "an-agent"),
  terminal("2", "/elsewhere/lab", "zsh"),
  terminal("3", "/somewhere/a-code-project", "zsh"),
];

function machineHolds(terminals: TerminalSummary[]) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        if (command === "terminal_list") return Promise.resolve(terminals);
        if (command === "terminal_backlog") return Promise.resolve({ at: 0, bytes: "", upto: 0, ended: null });
        if (command === "profile_command_lines") return Promise.resolve([]);
        return Promise.resolve([]);
      },
    },
    event: { listen: () => Promise.resolve(() => {}) },
  };
}

async function openTerminals() {
  render(<TheWindow native />);
  fireEvent.click(screen.getByRole("button", { name: "Terminals" }));
  await waitFor(() => { expect(document.querySelectorAll(".window-row")).toHaveLength(3); });
}

/** A row of the panel by what it names, and never the field's head. */
function row(group: number, at: number): HTMLElement {
  return document.querySelectorAll(".window-rows")[group].querySelectorAll<HTMLElement>(".window-row")[at];
}

describe("the terminals, in the three columns", () => {
  test("THE ROWS ARE GROUPED BY THE TREE each terminal was opened in", async () => {
    machineHolds(THREE);
    await openTerminals();
    const heads = [...document.querySelectorAll(".window-rows__head")].map((head) => head.textContent);
    expect(heads).toEqual(["a-code-project", "lab"]);
    const first = document.querySelectorAll(".window-rows")[0];
    expect([...first.querySelectorAll(".window-row__name")].map((name) => name.textContent)).toEqual(["an-agent", "zsh"]);
  });

  test("THE FIELD HOLDS ONE TERMINAL, and carries no tabs of its own", async () => {
    machineHolds(THREE);
    await openTerminals();
    await waitFor(() => { expect(document.querySelectorAll(".window-field .pane")).toHaveLength(1); });
    expect(screen.queryAllByRole("tab"), "a strip of tabs is a second navigation").toHaveLength(0);
    expect(document.querySelectorAll(".pane"), "a pane outside the field").toHaveLength(1);
    expect(document.querySelector(".window-field__head"), "a head over the pane's own head").toBeNull();
  });

  test("choosing a row puts that terminal in the field", async () => {
    machineHolds(THREE);
    await openTerminals();
    fireEvent.click(row(1, 0));
    await waitFor(() => {
      expect(document.querySelector(".window-field")?.getAttribute("aria-label")).toBe("zsh · lab");
    });
    expect(document.querySelector(".window-row[aria-current]")?.textContent).toContain("zsh");
  });

  test("with none open, the panel says so rather than drawing nothing", async () => {
    machineHolds([]);
    render(<TheWindow native />);
    fireEvent.click(screen.getByRole("button", { name: "Terminals" }));
    await waitFor(() => { expect(screen.getByText("No terminal is open on this machine.")).toBeDefined(); });
    expect(document.querySelector(".pane")).toBeNull();
  });
});

describe("a tree is its root, not its name", () => {
  test("two trees called the same word are headed by their paths", () => {
    const groups = terminalGroups([terminal("1", "/a/sailor", "zsh"), terminal("2", "/b/sailor", "zsh")]);
    expect(groups.map((group) => group.head)).toEqual(["/a/sailor", "/b/sailor"]);
  });
});
