// @vitest-environment jsdom
/**
 * **EVERY POWER THE WINDOW DECLARES IS ONE IT CAN REACH.** ⌘K was built from
 * the strip, and `changes` and `sketch` hang under the tree: no list typing
 * could read held them. What is expected here is read from the declarations.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { MACHINE, PLACES } from "./places";
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

  /** Listing is not reaching: a row that names a place must open it. */
  test("AND WHAT IT LISTS IS WHERE IT LANDS, for the two that were missing", () => {
    const { container } = render(<App />);
    const crumbs = () =>
      Array.from(container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);

    offered();
    fireEvent.click(screen.getByRole("option", { name: /^Changes/ }));
    expect(crumbs()[0], "«Changes» is in the palette and does not open the changes").toBe("Changes");

    offered();
    fireEvent.click(screen.getByRole("option", { name: /^Whiteboard/ }));
    expect(crumbs()[0], "«Whiteboard» is in the palette and does not open the whiteboard").toBe(
      "Whiteboard",
    );
  });

  /** The absurd control: the palette does not answer for what nobody declared. */
  test("and a place the window does not have is not offered", () => {
    render(<App />);
    expect(names(offered(), "A place nobody declared")).toBe(false);
  });
});
