// @vitest-environment jsdom
/**
 * **THE ORDINARY VIEW CARRIES NO MENU OF DESTINATIONS.** A permanent list of
 * where you could be is navigation holding space the work needs; sections and
 * configuration are reached by typing, and from the tree you stand in. What
 * this removes must stay reachable — the whole point is which of the two wins.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { MACHINE, MACHINE_GROUND, PLACES, UNDER_A_TREE } from "./places";

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

    type("Runs");
    expect(crumbs(), "the history is not reachable any more").toEqual(["Runs", "Runs"]);

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

    // Board, Changes and Whiteboard answer about a tree, and hang under one.
    for (const id of UNDER_A_TREE) {
      expect(PLACES.map((one) => one.id), `«${id}» is not a place any more`).toContain(id);
    }
    expect(screen.getByRole("button", { name: /^Board/ }), "no way to the board out here").toBeTruthy();
  });
});
