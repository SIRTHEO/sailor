// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";

/**
 * **THE BAR IS ALLOWED THREE FACTS, AND IT HELD SIX CONTROLS OF ONE SECTION.**
 * Where the flows came from, the flow's size, its unsaved mark, how it last
 * ran, Save and Run: all in the strip that never leaves the screen, about the
 * board, in sight from the other five places.
 */

/** What the board asked the engine to run, so the button can be proved wired. */
const asked = vi.hoisted(() => ({ runs: [] as string[] }));

vi.mock("./engine", async (importOriginal) => {
  const real = await importOriginal<typeof import("./engine")>();
  return {
    ...real,
    startRun: (name: string) => {
      asked.runs.push(name);
      return Promise.reject(new Error("this test does not run flows"));
    },
  };
});

/** A disk with no flows at all: the board with nothing in focus. */
const disk = vi.hoisted(() => ({ empty: false }));

vi.mock("./sample", async (importOriginal) => {
  const real = await importOriginal<typeof import("./sample")>();
  return {
    ...real,
    get SAMPLE() {
      return disk.empty ? [] : real.SAMPLE;
    },
  };
});

import App from "./App";
import stylesheetSource from "./styles.css?raw";

afterEach(cleanup);

beforeEach(() => {
  asked.runs = [];
  disk.empty = false;
  window.localStorage.clear();
});

/** Enough for React Flow to mount: the canvas is not the subject here. */
beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

function goToTheBoard(): void {
  fireEvent.click(screen.getByRole("button", { name: /^Board/ }));
}

describe("the bar carries no control of the board", () => {
  test("NOT SAVE, NOT RUN, NOT THE FLOW'S SIZE, NOT WHERE THE FLOWS CAME FROM", () => {
    const { container } = render(<App />);
    goToTheBoard();

    // The control that makes every absence below worth something: the board
    // really is drawn, with a flow in focus, so each thing does exist somewhere.
    expect(container.querySelector(".focusbar"), "no flow is in focus").not.toBeNull();

    const bar = container.querySelector(".topbar") as HTMLElement;
    const buttons = Array.from(bar.querySelectorAll("button")).map((one) => one.textContent ?? "");
    expect(buttons.filter((word) => /^(Save|Saving|▶?Run|Starting)/.test(word))).toEqual([]);
    expect(bar.querySelector("[data-source]"), "the bar says where the flows came from").toBeNull();
    expect(bar.textContent ?? "", "the bar counts the flow's steps").not.toMatch(/\bsteps\b/);
    expect(bar.textContent ?? "", "the bar carries the unsaved mark").not.toMatch(/unsaved/i);
  });

  test("AND THE BOARD'S HEAD DOES, WITH RUN STILL THE SAME GESTURE", async () => {
    const { container } = render(<App />);
    goToTheBoard();

    const head = container.querySelector(".boardhead") as HTMLElement;
    expect(head.querySelector(".boardhead__source")?.textContent).toBe("sample data");
    expect(head.querySelector(".focusbar__steps")?.textContent).toMatch(/\d+ steps/);
    expect(head.querySelector(".focusbar__status-word")?.textContent ?? "").not.toBe("");

    const name = head.querySelector(".focusbar__name")?.textContent ?? "";
    expect(name).not.toBe("");
    // A BUTTON THAT MOVED AND STOPPED WORKING IS NOT A BUTTON THAT MOVED.
    const run = head.querySelector(".focusbar__run") as HTMLButtonElement;
    fireEvent.click(run);
    await vi.waitFor(() => {
      expect(asked.runs).toEqual([name]);
    });
  });

  test("WHERE THE FLOWS CAME FROM STANDS WITH NO FLOW IN FOCUS", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToTheBoard();

    expect(container.querySelector(".focusbar"), "a flow is in focus after all").toBeNull();
    expect(container.querySelector(".boardhead__source")?.textContent).toBe("sample data");
  });

  /**
   * A sample and a mute engine are the two cases where somebody risks reading
   * as true what is not, and the bar hid both under 760px — the width where a
   * screen is least likely to be doubted.
   */
  test("AND NO WIDTH HIDES IT", () => {
    const hidden = /([^{}]*)\{[^{}]*display:\s*none[^{}]*\}/g;
    const hiding = Array.from(stylesheetSource.matchAll(hidden))
      .map((match) => match[1])
      .filter((selectors) => selectors.includes(".boardhead__source"));
    expect(hiding).toEqual([]);
  });
});
