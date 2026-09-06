// @vitest-environment jsdom
/**
 * **ONE PLACE TO CONSULT WHAT HAPPENED.** The tables sat beside the runs, so
 * one question — what did this machine do, and what did it cost — had two doors
 * and neither named the other. The tables are a view, opened without a run.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { MACHINE, PLACES } from "./places";
import { MEMORY_TABS } from "./memorytabs";

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

/** The section on screen: the terminals stay mounted behind it, hidden. */
const SHOWN = ".section:not([hidden]) ";

/** A SECTION IS A DYNAMIC IMPORT AWAY. It is fetched when the place asks for
 *  it, so a query fired in the same tick as the gesture finds the gap the
 *  fallback leaves and concludes the section is not there. */
async function theSectionArrives(): Promise<void> {
  await waitFor(() => {
    expect(document.querySelector(".section:not([hidden])")).toBeTruthy();
  });
}

/** The sections are typed for: the window carries no permanent menu of them. */
async function typeInThePalette(label: string): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
  const rows = Array.from(document.querySelectorAll<HTMLElement>(".palette__entry"));
  const row = rows.find((one) => one.querySelector(".palette__label")?.textContent === label);
  expect(row, `the palette does not offer «${label}»`).toBeDefined();
  fireEvent.click(row as HTMLElement);
  await theSectionArrives();
}

/** A row of the section's own column, by the name it shows. */
function inTheColumn(root: Element, name: string): HTMLElement {
  const rows = Array.from(root.querySelectorAll<HTMLElement>(`${SHOWN}.subrail__item`));
  const row = rows.find((one) => one.querySelector(".subrail__name")?.textContent === name);
  expect(row, `the history's column has no «${name}»`).toBeDefined();
  return row as HTMLElement;
}

describe("the history is one place", () => {
  test("THE TABLES ARE A VIEW INSIDE THE RUNS, not a section beside them", async () => {
    const { container } = render(<App />);
    await typeInThePalette("Runs");

    const inside = Array.from(container.querySelectorAll(`${SHOWN}.subrail__name`)).map(
      (one) => one.textContent,
    );
    expect(inside, "the tables are not a view of the history").toContain("Ledger");
    expect(inside).toEqual(MEMORY_TABS.map((one) => one.name));

    // The control: no section of the window is the ledger any more.
    expect(PLACES.map((one) => one.id)).not.toContain("ledger");
  });

  test("AND THEY OPEN WITHOUT PASSING THROUGH A SINGLE RUN", async () => {
    const { container } = render(<App />);
    await typeInThePalette("Runs");
    // Straight from the history's own column, with no run picked first.
    fireEvent.click(inTheColumn(container, "Ledger"));

    expect(container.querySelector(`${SHOWN}.browser`), "the tables did not open").not.toBeNull();
    const crumbs = Array.from(container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);
    expect(crumbs, "the bar does not say the tables are part of the history").toEqual(["Runs", "Ledger"]);
  });

  test("AND THE MACHINE'S OWN ROW LANDS IN THAT SAME PLACE", async () => {
    // «The ledger has one place of consultation»: the row on the machine's
    // ground is a way in, not a second copy of the screen.
    const row = MACHINE.find((one) => one.id === "ledger");
    expect(row?.section, "the machine's ledger row still opens a section of its own").toBe("memory");
    expect(row?.memoryTab).toBe("ledger");

    const { container } = render(<App />);
    await typeInThePalette("Runs");
    fireEvent.click(inTheColumn(container, "Faults"));
    await typeInThePalette(row?.name ?? "");

    expect(container.querySelector(`${SHOWN}.browser`), "the machine's row did not open the tables").not.toBeNull();
  });
});
