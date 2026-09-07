// @vitest-environment jsdom
/**
 * **THE MACHINE FILLING UP WAS NOTICED IN THE WINDOW AND ANSWERED NOWHERE IN
 * IT.** What this screen must never do is offer to stop something somebody is
 * waiting on: alive-and-wanted looks exactly like alive-and-forgotten unless
 * the run behind it is asked about, and the gesture here acts on that answer.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { RunningScreen } from "./RunningScreen";
import { standingOf, upFor, type Standing } from "./running";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

function row(over: Partial<Standing>): Standing {
  return {
    pid: 4321,
    purpose: "live",
    command: "npx",
    still_there: true,
    run_id: "a-run",
    run_is_over: true,
    started_at: 1_700_000_000,
    ...over,
  };
}

/** The shell, answering the reading and remembering what it was asked to stop. */
function answering(rows: Standing[]): { asked: string[] } {
  const asked: string[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (name: string) => {
        asked.push(name);
        return Promise.resolve(name === "what_sailor_lit" ? rows : []);
      },
    },
  };
  return { asked };
}

describe("what Sailor is running", () => {
  test("a run that ended and a process still up is what fills a machine, and is named so", () => {
    expect(standingOf(row({}))).toBe("leftover");
    expect(standingOf(row({ run_is_over: false }))).toBe("wanted");
    expect(standingOf(row({ still_there: false }))).toBe("ended");
  });

  /**
   * **NO RUN IS NOT «NOBODY WANTS IT».** It means nobody wrote down who did,
   * and read as abandonment it turns a process somebody started by hand into
   * something the window offers to kill.
   */
  test("a row no run ever claimed is its own case, not a leftover", () => {
    expect(standingOf(row({ run_id: null }))).toBe("unclaimed");
    expect(standingOf(row({ run_id: null }))).not.toBe("leftover");
  });

  test("the rows arrive with what each is for and how long it has been up", async () => {
    answering([row({ purpose: "live", command: "npx", pid: 51 })]);
    render(<RunningScreen native />);
    await waitFor(() => expect(screen.getByText("live")).toBeTruthy());
    expect(screen.getByText("npx")).toBeTruthy();
    expect(screen.getByText("51")).toBeTruthy();
  });

  /** The gesture names the number it will stop, and only the leftovers count. */
  test("only what a finished run left behind is offered for stopping", async () => {
    answering([
      row({ pid: 1, run_is_over: true }),
      row({ pid: 2, run_is_over: false }),
      row({ pid: 3, run_id: null }),
      row({ pid: 4, still_there: false }),
    ]);
    render(<RunningScreen native />);
    const button = await screen.findByRole("button");
    expect(button.textContent).toContain("1");
    expect(button.textContent).not.toContain("4");
  });

  /**
   * **A BUTTON THAT WOULD DO NOTHING MUST NOT INVITE A PRESS**, or the screen
   * teaches that pressing it has no effect, which is the wrong lesson on the
   * day there is something to stop.
   */
  test("with nothing left over the gesture is shut and says so", async () => {
    answering([row({ run_is_over: false })]);
    render(<RunningScreen native />);
    const button = await screen.findByRole("button");
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(button.textContent).toContain("nothing here to free");
  });

  /** And a process that would not leave keeps its row, and says why. */
  test("a process that would not stop is reported, not called dead", async () => {
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (name: string) =>
          Promise.resolve(
            name === "what_sailor_lit"
              ? [row({ pid: 77, purpose: "live" })]
              : [{ pid: 77, purpose: "live", stopped: false, why: "it was asked to leave and did not" }],
          ),
      },
    };
    render(<RunningScreen native />);
    const button = await screen.findByRole("button");
    button.click();
    await waitFor(() =>
      expect(screen.getByText(/would not stop: it was asked to leave and did not/)).toBeTruthy(),
    );
  });

  test("how long it has been up reads in the units a person uses", () => {
    expect(upFor(100, 130)).toBe("30 s");
    expect(upFor(0, 3 * 60)).toBe("3 min");
    expect(upFor(0, 3 * 3600 + 60)).toBe("3 h 1 min");
    expect(upFor(0, 3 * 86_400)).toBe("3 d 0 h");
    // A row written by a clock ahead of this one is not shown as negative.
    expect(upFor(100, 50)).toBe("0 s");
  });
});
