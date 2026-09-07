// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { World } from "./World";

/**
 * **A FIXTURE READ AS YOUR OWN WORK.** Outside the shell the column lists
 * invented flows, one broken on purpose, and the declaration lived on the
 * board's head — not on screen unless you were on the board.
 */

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() { /* the canvas measures nothing here */ }
    unobserve() { /* nor stops */ }
    disconnect() { /* nor lets go */ }
  };
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: false, media: query, onchange: null,
      addListener: () => undefined, removeListener: () => undefined,
      addEventListener: () => undefined, removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }),
  });
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("flows nobody wrote", () => {
  test("THE COLUMN THAT LISTS THEM SAYS THEY ARE NOT YOURS", () => {
    const { container } = render(<App />);
    const said = container.querySelector(".world__sample")?.textContent ?? "";
    expect(said, "the column lists invented flows and declares nothing").not.toBe("");
    expect(said.toLowerCase()).toContain("sample");
    // The control: it says so beside the flows it is about, in the column, and
    // not on a screen you have to walk to.
    expect(
      container.querySelector(".world .world__sample"),
      "the declaration is somewhere other than the list it is about",
    ).not.toBeNull();
  });

  test("AND FLOWS THAT ARE REALLY THERE ARE NOT CALLED INVENTED", () => {
    const { container } = render(
      <World
        native
        source="engine"
        here="board"
        onGo={() => undefined}
        counts={{ board: 1 }}
        terminals={[]}
        onMoved={() => undefined}
        onTree={() => undefined}
        onNewFlow={() => undefined}
        flowGroups={[
          {
            origin: "this project",
            flows: [{ name: "a-flow", note: "7 steps", color: "#4ea7fc", dirty: false, live: null }],
            broken: [],
          },
        ]}
        focusName={null}
        onFlow={() => undefined}
      />,
    );
    expect(
      container.querySelector(".world__sample"),
      "work read off the disk was called sample data",
    ).toBeNull();
  });

  test("and the invented broken flow is one of the things it is about", () => {
    render(<App />);
    expect(screen.getByText("notte"), "the fixture this was written for is gone").toBeTruthy();
  });
});
