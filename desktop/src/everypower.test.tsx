// @vitest-environment jsdom
/**
 * **EVERY POWER THE WINDOW DECLARES IS ONE IT CAN REACH.** ⌘K was built from
 * the strip, and `changes` and `sketch` hang under the tree: no list typing
 * could read held them. What is expected here is read from the declarations.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { onItsOwnName, MACHINE, PLACES , type Section } from "./places";
import { SAILOR_TABS } from "./sailortabs";
import { MEMORY_TABS } from "./memorytabs";
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

/** The labels ⌘K offers, with the palette open over whatever is on screen. */
function offered(): string[] {
  fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
  return Array.from(document.querySelectorAll(".palette__label")).map(
    (one) => one.textContent ?? "",
  );
}

/** A label is that name, or a name under a section: «Runs › Faults». */
function names(labels: string[], name: string): boolean {
  return labels.some((label) => label === name || label.endsWith(`› ${name}`));
}

describe("what ⌘K can reach", () => {
  test("EVERY SECTION THE WINDOW DECLARES HAS A ROW IN THE PALETTE", () => {
    render(<App />);
    const labels = offered();

    for (const place of PLACES) {
      // A section the machine's ground names better is named by that row. Not
      // by a row that opens ONE VIEW inside it: «Ledger» reaches the tables,
      // not the runs beside them, and counting it hid the section behind it.
      const named = [
        place.name,
        ...MACHINE.filter((row) => row.section === place.id && row.memoryTab === undefined).map(
          (row) => row.name,
        ),
      ];
      expect(
        named.some((name) => names(labels, name)),
        `«${place.name}» is a section of this window and ⌘K cannot reach it`,
      ).toBe(true);
    }
  });

  test("AND EVERY TAB INSIDE A SECTION, machine, history and terminals alike", () => {
    render(<App />);
    const labels = offered();

    for (const tab of SAILOR_TABS) {
      const row = MACHINE.find((one) => one.tab === tab.id);
      expect(row, `the tab «${tab.name}» has no row on the machine's ground`).toBeDefined();
      expect(names(labels, row?.name ?? ""), `«${tab.name}» is not in the palette`).toBe(true);
    }
    for (const tab of MEMORY_TABS) {
      expect(names(labels, tab.name), `«${tab.name}» is not in the palette`).toBe(true);
    }
    for (const tab of TERMINALS_TABS) {
      expect(names(labels, tab.name), `«${tab.name}» is not in the palette`).toBe(true);
    }
  });

  /**
   * Listing is not reaching, and **A ROW THAT OPENS ON NOTHING IS THE DEFECT
   * THIS GUARD EXISTS FOR**: naming two places by hand could not see a third
   * added later. Board and the work stay mounted behind the rest, and their
   * own guards measure them.
   */
  test("AND EVERY ROW IT LISTS OPENS SOMETHING, not only the two that were missing", () => {
    const drawn: Section[] = ["board", "terminals"];
    for (const place of onItsOwnName().filter((one) => !drawn.includes(one.id))) {
      cleanup();
      const { container } = render(<App />);
      offered();
      const row = screen
        .getAllByRole("option")
        .find((one) => one.querySelector(".palette__label")?.textContent === place.name);
      expect(row, `«${place.name}» is a place the palette does not offer`).toBeDefined();
      fireEvent.click(row as HTMLElement);

      const crumb = container.querySelector(".topbar__crumb")?.textContent;
      expect(crumb, `«${place.name}» is offered and does not open`).toBe(place.name);
      // Asked of the section this place opened, never of «a section»: the
      // work and the board stay mounted behind it, and their bodies answered
      // for a place that had drawn nothing at all.
      const body = container.querySelector(`[data-place="${place.id}"] .section__body`);
      expect(body, `«${place.name}» opens a place with no body`).not.toBeNull();
      expect(
        (body?.textContent ?? "").trim().length,
        `«${place.name}» opens on an empty stage`,
      ).toBeGreaterThan(0);
    }
  });

  /** The absurd control: the palette does not answer for what nobody declared. */
  test("and a place the window does not have is not offered", () => {
    render(<App />);
    expect(names(offered(), "A place nobody declared")).toBe(false);
  });
});
