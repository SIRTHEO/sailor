// @vitest-environment jsdom
/**
 * **«SAILOR» IS NOT A DESTINATION.** A section named after the program answered
 * «what it knows, what it can do» — nine screens folded into one noun that says
 * nothing about any of them. What this machine holds is reached by name, as an
 * explicit machine access, and the window still reopens on the screen you left.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { MACHINE, MACHINE_GROUND, PLACES, SECTIONS } from "./places";

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

/** Every row ⌘K offers, as the group it is filed under and its label. */
function offered(): { group: string; label: string }[] {
  fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
  let group = "";
  const rows: { group: string; label: string }[] = [];
  for (const one of document.querySelectorAll(".palette__group, .palette__label")) {
    if (one.className === "palette__group") group = one.textContent ?? "";
    else rows.push({ group, label: one.textContent ?? "" });
  }
  return rows;
}

describe("what this machine holds is reached by name", () => {
  test("NO SECTION IS NAMED AFTER THE PROGRAM ITSELF", () => {
    expect(PLACES.map((one) => one.name), "«Sailor» is still a place a person picks").not.toContain(
      "Sailor",
    );
    // And the ground it stood for is still a ground: dropping the noun must not
    // drop the nine screens with it.
    expect(MACHINE.length).toBeGreaterThan(8);
  });

  test("EVERY MACHINE ROW IS AN EXPLICIT ACCESS IN THE PALETTE, filed under the machine", () => {
    render(<App />);
    const rows = offered();
    for (const row of MACHINE) {
      const found = rows.find((one) => one.label === row.name);
      expect(found, `«${row.name}» is not in the palette`).toBeDefined();
      expect(
        found?.group,
        `«${row.name}» is offered as somewhere to go, not as a machine access`,
      ).toBe(MACHINE_GROUND);
    }
  });

  /**
   * **NOTHING DECIDES WHERE THE DAY BEGINS EXCEPT WHERE YOU LEFT OFF.** The
   * section a machine screen lives in is not one of the offered places, and a
   * restore that reads the offered places would land everybody on the board.
   */
  test("AND THE WINDOW REOPENS ON THE MACHINE SCREEN IT WAS LEFT ON", () => {
    const first = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /^Models/ }));
    expect(
      Array.from(first.container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent),
    ).toEqual([MACHINE_GROUND, "Models"]);
    cleanup();

    // A NEW WINDOW, not a re-render: this is what a build leaves behind.
    const again = render(<App />);
    expect(
      Array.from(again.container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent),
      "it opened on the board again, and the walk back is the cost",
    ).toEqual([MACHINE_GROUND, "Models"]);
  });

  /** The control: the sections a place list no longer names are still sections. */
  test("and every section a row lands on is one the window can come back to", () => {
    for (const row of MACHINE) {
      expect(SECTIONS, `nothing can reopen on «${row.name}»`).toContain(row.section);
    }
    for (const place of PLACES) {
      expect(SECTIONS).toContain(place.id);
    }
  });
});
