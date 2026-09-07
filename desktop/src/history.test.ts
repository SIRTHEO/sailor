import { describe, expect, test } from "vitest";

import { lastedOf, outcomeOf, runsIn, whenOf } from "./History";
import { countByFamily } from "./Installed";
import type { Execution, InstalledEntry } from "./engine";
import type { TokenTotals } from "./flow";

/** Named, never spelled: a fixture written out by hand drifts from the wire. */
const NOTHING_SPENT: TokenTotals = {
  input_tokens: 0,
  output_tokens: 0,
  cached_tokens: 0,
  cache_write_tokens: 0,
  total_tokens_only: 0,
  cost_micros: 0,
  calls: 0,
  turns: 0,
  calls_without_tokens: 0,
  calls_without_cost: 0,
};

/**
 * **THE RULES THAT DECIDE THE COLOUR OF A ROW.**
 *
 * Getting them wrong drops nothing: the table draws all the same, and whoever
 * reads it believes what they see. A broken run counted among the open ones
 * leaves the eye of whoever hunts faults, and that is the costliest possible
 * fault in a view that exists to find them.
 */

function run(over: Partial<Execution>): Execution {
  return {
    run_id: "r",
    kind: "flow",
    entity: "sviluppa-sailor",
    worktree: null,
    status: "succeeded",
    started_at: 0,
    ended_at: 1,
    duration_secs: 1,
    total_cost_micros: 0,
    error: null,
    steps_total: 3,
    steps_went: 3,
    steps_broke: 0,
    steps_retried: 0,
    steps_open: [],
    tokens: NOTHING_SPENT,
    tokens_by_model: {},
    calls: [],
    ...over,
  };
}

describe("outcomeOf", () => {
  test("ROTTA VINCE SU APERTA", () => {
    // A run with one step down and another still in flight is a fault that is
    // still burning. Counting it among the open ones would take it out of the
    // eye of whoever watches faults — and that is the line this test defends.
    const both = run({
      status: "running",
      steps_broke: 1,
      steps_open: [{ step_id: "prove", attempt: 1, started_at: 0, open_for_secs: 30 }],
    });
    expect(outcomeOf(both)).toBe("broke");
  });

  test("a written error is enough, even with no step fallen", () => {
    expect(outcomeOf(run({ error: "the ledger does not answer", steps_broke: 0 }))).toBe("broke");
  });

  test("a state we do not know does not become «went»", () => {
    // «other» is ugly to read and honest: inventing a success on a state the
    // engine never declared is how a view starts to lie with nobody there to
    // contradict it.
    expect(outcomeOf(run({ status: "waiting", steps_open: [] }))).toBe("other");
  });

  test("the attempts do not change the outcome", () => {
    // A step repeated and then successful is a run that went, and the effort
    // reads in the retried column instead of in a red that is not there.
    expect(outcomeOf(run({ steps_retried: 2 }))).toBe("went");
  });
});

describe("lastedOf", () => {
  test("a run that never ended does not last zero", () => {
    // `0 s` on a run that never ended is the convenient lie: it looks
    // instantaneous rather than interrupted.
    expect(lastedOf(null)).toBe("—");
  });

  test("the durations climb a scale instead of getting longer", () => {
    expect(lastedOf(45)).toBe("45 s");
    expect(lastedOf(90)).toBe("1 min");
    expect(lastedOf(3 * 3600 + 25 * 60)).toBe("3 h 25 min");
  });
});

describe("whenOf", () => {
  test("today writes only the time, another day carries the date", () => {
    const now = new Date(2026, 7, 31, 21, 0, 0).getTime() / 1000;
    const thisMorning = new Date(2026, 7, 31, 9, 5, 0).getTime() / 1000;
    const yesterday = new Date(2026, 7, 30, 9, 5, 0).getTime() / 1000;
    expect(whenOf(thisMorning, now)).toBe("09:05");
    expect(whenOf(yesterday, now)).toBe("30/08 09:05");
  });
});

describe("countByFamily", () => {
  test("the families at zero stay in the list", () => {
    // A family that disappears when empty makes you believe it does not exist:
    // whoever has yet to write a hook must see «hooks 0», not silence.
    const entries: InstalledEntry[] = [
      { kind: "skill", name: "a", description: "", origin: "home", path: "/a", reach: { state: "active" }, by_model: true },
      { kind: "skill", name: "b", description: "", origin: "home", path: "/b", reach: { state: "active" }, by_model: true },
      { kind: "hook", name: "c", description: "", origin: "home", path: "/c", reach: { state: "active" }, by_model: false },
    ];
    expect(countByFamily(entries)).toEqual({ skill: 2, agent: 0, command: 0, rule: 0, hook: 1 });
  });
});

/**
 * **EVERY TREE'S RUNS IN ONE LIST IS A LIST ABOUT NOBODY.** `runs.worktree` is
 * what makes the narrower question askable at all; a run recorded before that
 * column has no answer, and must never be counted into a tree it might not
 * belong to.
 */
describe("the runs of one tree", () => {
  const mine = run({ run_id: "mine", worktree: "/t/a-project/a-tree" });
  const theirs = run({ run_id: "theirs", worktree: "/t/a-project/another" });
  const older = run({ run_id: "older", worktree: null });

  test("a tree gets its own, and nothing it cannot claim", () => {
    expect(runsIn([mine, theirs, older], "/t/a-project/a-tree").map((one) => one.run_id)).toEqual([
      "mine",
    ]);
  });

  test("and asking about no tree asks about all of them", () => {
    expect(runsIn([mine, theirs, older], null)).toHaveLength(3);
  });
});
