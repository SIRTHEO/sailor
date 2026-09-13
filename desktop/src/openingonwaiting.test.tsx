// @vitest-environment jsdom
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test } from "vitest";

// **SOMETHING WAITING OUTRANKS WHERE YOU LEFT OFF.** `whereyouwere.test.ts`
// already proves a recent place survives a rebuild; this is the one case
// that must override it — a parked task must not sit behind the board a
// person happened to close the window on. `App` is imported dynamically,
// after `window.__TAURI__` is declared: `NATIVE` is read once, at module
// load, so the shell must exist before the very first import of `./App`.

/** Enough for React Flow to mount: the canvas is not the subject here. */
class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
beforeEach(() => window.localStorage.clear());

function pretendNativeShell(answers: Record<string, unknown>) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        if (!(command in answers)) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(answers[command]);
      },
    },
  };
}

describe("what waits for you outranks the place you left", () => {
  test("A FRESH LAUNCH WITH SOMETHING WAITING OPENS THERE, even over a recent place", async () => {
    window.localStorage.setItem(
      "sailor.where",
      JSON.stringify({ place: "board", at: Math.floor(Date.now() / 1000) }),
    );
    pretendNativeShell({
      attention_queue: [
        {
          kind: "handed",
          run_id: "run-1",
          step_id: "review",
          status_word: "waiting on you",
          reason: "the acceptance check did not pass",
          since: 0,
        },
      ],
    });

    const { default: App } = await import("./App");
    const { container } = render(<App />);

    await waitFor(() => {
      expect(container.querySelector('.section[data-place="waiting"]')).not.toBeNull();
    });
  });
});
