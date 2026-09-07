// @vitest-environment jsdom
import { describe, expect, test } from "vitest";
import { panesFromEvents, readWhy } from "./RunConsole";
import type { RunEvent } from "./engine";

/**
 * **THE REASON ARRIVES WITH THE INPUT, AT OPEN.** A step the engine skipped
 * closes with no text, so its reason is the only thing there is to read; and a
 * step that never closes is exactly the one somebody asks about.
 */

function started(why: unknown): RunEvent {
  return {
    run_id: "r",
    seq: 1,
    kind: "step_started",
    at: 1,
    step_id: "publish",
    payload: { attempt: 1, input: {}, why },
  };
}

const WHOLE = {
  because: "condition",
  held: false,
  looked_at: "/report/status",
  wanted: 'equals "failed"',
  found: "passed",
  found_was_there: true,
};

describe("the reason of a step being watched live", () => {
  test("REACHES THE PANE FROM THE OPENING EVENT, before any step closes", () => {
    const [pane] = panesFromEvents([started(WHOLE)]);
    expect(pane?.why?.looked_at).toBe("/report/status");
    expect(pane?.why?.held).toBe(false);
    expect(pane?.endedAt, "the pane is still open: the reason did not wait for the close").toBeNull();
  });

  test("A HALF-SHAPED REASON IS NO REASON", () => {
    expect(readWhy({ because: "condition", held: false }), "no demand: it judged nothing").toBeNull();
    expect(readWhy({ held: false, wanted: "x" }), "no species of reason at all").toBeNull();
    expect(readWhy({ because: "condition", held: "no", wanted: "x" }), "a verdict that is not one").toBeNull();
    expect(readWhy(null)).toBeNull();
    expect(readWhy("skipped")).toBeNull();
  });

  test("and nothing found survives the wire as nothing, not as null", () => {
    const absent = readWhy({ because: "condition", held: false, looked_at: "/a", wanted: "is there at all" });
    expect(absent?.found_was_there).toBe(false);
    const empty = readWhy({ ...WHOLE, found: null, found_was_there: true });
    expect(empty?.found_was_there).toBe(true);
  });

  test("a condition that reads no place is still a whole reason", () => {
    const why = readWhy({ because: "condition", held: true, wanted: "the input equals 3", found: 3, found_was_there: true });
    expect(why?.looked_at, "no place read is not a missing field").toBeNull();
    expect(why?.held).toBe(true);
  });
});
