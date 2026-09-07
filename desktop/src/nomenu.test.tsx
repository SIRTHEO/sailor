// @vitest-environment jsdom
/**
 * **THE ORDINARY VIEW CARRIES NO MENU OF DESTINATIONS.** A permanent list of
 * where you could be holds space the work needs; sections are reached by
 * typing. What it removes must stay reachable, or the removal is a loss.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { nameOfPlace, MACHINE, MACHINE_GROUND, PLACES, UNDER_A_TREE , UNDER_THE_TREE } from "./places";

/** Read, never spelled: a hard-coded name breaks on a rename. */
const WHY = nameOfPlace("memory");

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

describe("the permanent menu is out of the ordinary view", () => {
  test("NEITHER THE STRIP OF PLACES NOR THE MACHINE'S NINE ROWS ARE ON SCREEN", () => {
    const { container } = render(<App />);

    expect(container.querySelector(".world__above"), "the strip of places is still drawn").toBeNull();
    const heads = Array.from(container.querySelectorAll(".world__head")).map((one) => one.textContent);
    expect(heads, "what this machine holds is still a permanent ground").not.toContain(MACHINE_GROUND);
    for (const row of MACHINE) {
      expect(
        container.querySelector(`.world__global[title="${row.asks}"]`),
        `«${row.name}» is still a permanent row of the column`,
      ).toBeNull();
    }
  });

  /** Removing a way in is only allowed because another one answers for it. */
  test("AND WHAT IT REMOVED IS REACHED BY TYPING, and lands", () => {
    const { container } = render(<App />);
    const crumbs = () =>
      Array.from(container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);
    // By the label the palette draws, not by the accessible name: that one
    // carries the hint too, and «Runs» is a place and a view inside it.
    const type = (label: string) => {
      fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
      const rows = Array.from(document.querySelectorAll<HTMLElement>(".palette__entry"));
      const row = rows.find((one) => one.querySelector(".palette__label")?.textContent === label);
      expect(row, `the palette does not offer «${label}»`).toBeDefined();
      fireEvent.click(row as HTMLElement);
    };

    type(WHY);
    expect(crumbs(), "the history is not reachable any more").toEqual([WHY, "Runs"]);

    type("Profiles");
    expect(crumbs(), "what this machine holds is not reachable any more").toEqual([
      MACHINE_GROUND,
      "Profiles",
    ]);
  });

  /** The control: the tree you stand in is context, not a menu of the program. */
  test("and the tree you stand in keeps the places that are about it", () => {
    const { container } = render(<App />);
    const heads = Array.from(container.querySelectorAll(".world__head")).map((one) => one.textContent);
    expect(heads).toEqual(["workspaces", "flows everywhere", "outside every workspace"]);

    // A fixed row would mean one tree while you stand in another.
    for (const id of UNDER_A_TREE) {
      expect(
        UNDER_THE_TREE.map((one) => one.id),
        `«${id}» hangs under no tree any more`,
      ).toContain(id);
      expect(
        PLACES.map((one) => one.id),
        `«${id}» is offered twice: once under the tree and once in the list`,
      ).not.toContain(id);
    }
    expect(screen.getByRole("button", { name: /^Board/ }), "no way to the board out here").toBeTruthy();
  });

  /**
   * **A QUESTION YOU HAVE TO KNOW THE NAME OF IS NOT OFFERED.** Held behind the
   * palette alone, the places cost a person the name of a place they had not
   * seen yet — the verdict on that shape was «ingestibile».
   */
  test("AND THE QUESTIONS ARE IN THE COLUMN, not only behind the palette", () => {
    const { container } = render(<App />);
    const inTheColumn = Array.from(
      container.querySelectorAll(".world__place .world__label"),
    ).map((one) => one.textContent);

    expect(inTheColumn, "the column offers no question at all").not.toEqual([]);
    for (const place of PLACES) {
      expect(inTheColumn, `«${place.name}» is reachable only by typing`).toContain(place.name);
    }
  });
});
