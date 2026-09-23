// @vitest-environment jsdom
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test } from "vitest";
import App from "./App";

/**
 * **THE THREE COLUMNS ARE THE WHOLE WINDOW.** Drawn inside this shell they were
 * surrounded by two navigations the design does not have, and the window a
 * person saw had five columns where it has three. Only a picture said so.
 */

afterEach(cleanup);

class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

beforeEach(() => {
  window.localStorage.clear();
});

/**
 * **THE CHUNK HAS TO ARRIVE BEFORE THE DOM CAN BE READ.** What this file looks
 * at lives behind the Suspense around `SailorScreen`, and that chunk now
 * carries the window, the pane and xterm: the default 1000 ms runs out while it
 * is still in flight, and the check goes red with the product unchanged.
 */
const CHUNK_ARRIVES = { timeout: 5000 };

/** Where the window was left, which is how it opens without a click. */
function leftAt(sailorTab: string) {
  window.localStorage.setItem(
    "sailor.where",
    JSON.stringify({ place: "sailor", sailorTab, at: Math.floor(Date.now() / 1000) }),
  );
}

describe("what draws around the three columns", () => {
  test("nothing, when the window is the thing open", async () => {
    leftAt("window");
    const { container } = render(<App />);
    await waitFor(() => { expect(container.querySelector(".window-shell")).not.toBeNull(); }, CHUNK_ARRIVES);
    expect(container.querySelector(".topbar"), "the bar and its crumbs draw over the field").toBeNull();
    expect(container.querySelector(".world"), "the column of places draws beside the column of lists").toBeNull();
  });

  // The other half of the same fact: a guard that hides the shell everywhere
  // would pass the test above and leave no window at all.
  test("both of them, on any other screen of the machine", async () => {
    leftAt("look");
    const { container } = render(<App />);
    await waitFor(() => { expect(container.querySelector(".world")).not.toBeNull(); }, CHUNK_ARRIVES);
    expect(container.querySelector(".topbar")).not.toBeNull();
    expect(container.querySelector(".window-shell")).toBeNull();
  });

  /** ⌘K IS THE WAY OUT, and it is bound to the window and not to the bar: with
   *  the bar gone it is the only one, so it is the one that has to be proved. */
  test("the palette still opens with the bar gone", async () => {
    leftAt("window");
    const { container } = render(<App />);
    await waitFor(() => { expect(container.querySelector(".window-shell")).not.toBeNull(); }, CHUNK_ARRIVES);
    expect(container.querySelector(".palette"), "it is open before the key").toBeNull();
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await waitFor(() => { expect(container.querySelector(".palette")).not.toBeNull(); });
  });
});
