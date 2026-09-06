// @vitest-environment jsdom
/**
 * **THE WINDOW IS THE TERMINALS; THE SECTIONS OPEN INSIDE THAT WORK.** As one
 * destination among seven the centre of the work was subordinate to navigation
 * and every rebuild chose it again. It is the ground, and it comes back.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { PLACES, TERMINALS_GROUND } from "./places";
import { TERMINALS_TABS } from "./TerminalsSection";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

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

function crumbsOf(root: Element): (string | null)[] {
  return Array.from(root.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);
}

/** The sections are typed for: the window carries no permanent menu of them. */
function typeInThePalette(label: string): void {
  fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
  const rows = Array.from(document.querySelectorAll<HTMLElement>(".palette__entry"));
  const row = rows.find((one) => one.querySelector(".palette__label")?.textContent === label);
  expect(row, `the palette does not offer «${label}»`).toBeDefined();
  fireEvent.click(row as HTMLElement);
}

describe("the terminals are the ground of the window", () => {
  test("AT REST THE WORK IS ON SCREEN, and it is no longer a place to choose", () => {
    const { container } = render(<App />);

    expect(
      container.querySelector(".section--sessions[hidden]"),
      "the window opens on something other than the work",
    ).toBeNull();
    expect(container.querySelector(".body[hidden]"), "the board is the ground again").not.toBeNull();
    expect(crumbsOf(container)).toEqual([TERMINALS_GROUND, "Live"]);

    // A destination is something a list offers. This one is not offered: it is
    // where you already are.
    expect(PLACES.map((one) => one.id)).not.toContain("terminals");
    expect(PLACES.map((one) => one.name)).not.toContain("Terminals");
  });

  /**
   * **NOTHING GIVES UP SPACE TO THE TERMINAL.** A panel beside the work is a
   * panel that squeezes it; a section opens in the stage the work holds, and
   * only one of them is in view at a time.
   */
  test("AND NOTHING SPLITS THE STAGE WITH IT", () => {
    const { container } = render(<App />);
    expect(container.querySelectorAll(".section:not([hidden])")).toHaveLength(1);

    typeInThePalette("Runs");
    expect(
      container.querySelectorAll(".section:not([hidden])"),
      "a second section took part of the stage the terminals hold",
    ).toHaveLength(1);
  });

  /**
   * **NO NOVELTY DECIDES WHERE THE DAY BEGINS.** A ground that always wins at
   * open is the same regression as a board that always won: what was left open
   * is what comes back, the ground included.
   */
  test("THE ARRANGEMENT LEFT COMES BACK, section and view alike", () => {
    render(<App />);
    typeInThePalette("Runs");
    cleanup();

    // A NEW WINDOW, not a re-render: this is what a build leaves behind.
    const onRuns = render(<App />);
    expect(crumbsOf(onRuns.container)[0], "the ground decided where the day begins").toBe("Runs");
    cleanup();

    // And back the other way: the ground is remembered like any other place,
    // and typing reaches it from wherever the window reopened.
    render(<App />);
    typeInThePalette("Live");
    cleanup();
    expect(crumbsOf(render(<App />).container)[0]).toBe(TERMINALS_GROUND);
  });

  test("and every view of the ground is still reached by typing", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
    const labels = Array.from(document.querySelectorAll(".palette__label")).map(
      (one) => one.textContent,
    );
    for (const tab of TERMINALS_TABS) {
      expect(labels, `«${tab.name}» is not in the palette`).toContain(tab.name);
    }
  });
});
