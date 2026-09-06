import { describe, expect, test } from "vitest";
import { groupRuns, howLong } from "./Now";
import type { OpenRun } from "./engine";

/**
 * **THE TWO THINGS THE FIRST SCREEN CAN GET WRONG IN SILENCE.**
 *
 * A duration written badly and a grouping that loses a row make no noise: the
 * screen draws all the same, and whoever looks believes what they read. They
 * are exactly the two faults the survey found public and confirmed in the other
 * products — a badge counting 1 while the list shows ten, a latency of 9.3
 * seconds written «9.3K».
 */

function run(over: Partial<OpenRun>): OpenRun {
  return {
    run_id: "r1",
    entity: "sviluppa-sailor",
    state: "working",
    open_steps: 1,
    open_now: [],
    since: 0,
    started_here: false,
    steps_done: 0,
    steps_total: null,
    ...over,
  };
}

describe("howLong", () => {
  test("arrotonda per difetto, non per eccesso", () => {
    // Two hours fifty: «3 h» would make someone step in on a thing that is not
    // yet what it looks like.
    expect(howLong(2 * 3600 + 50 * 60)).toBe("2 h 50 min");
    expect(howLong(119)).toBe("1 min");
  });

  test("i secondi restano secondi finché sono secondi", () => {
    expect(howLong(0)).toBe("0 s");
    expect(howLong(59)).toBe("59 s");
    expect(howLong(60)).toBe("1 min");
  });

  test("oltre il giorno non si scrive in ore", () => {
    // 50 h written «50 h» reads as two days only by counting: whoever looks
    // must see that a run has been open since yesterday without arithmetic.
    expect(howLong(50 * 3600)).toBe("2 d 2 h");
  });

  test("un tempo negativo non diventa un numero enorme", () => {
    // The clocks do not agree with each other: the ledger writes one instant,
    // the window reads another, and the difference can come out negative. A
    // «-3 s» is ugly, but «18446744073709 s» is a broken screen.
    expect(howLong(-3)).toBe("—");
  });
});

describe("groupRuns", () => {
  test("nessuna corsa si perde per strada", () => {
    const runs = [
      run({ run_id: "a", state: "waiting" }),
      run({ run_id: "b", state: "working" }),
      run({ run_id: "c", state: "waiting" }),
    ];
    const { waiting, working } = groupRuns(runs);
    expect(waiting.map((entry) => entry.run_id)).toEqual(["a", "c"]);
    expect(working.map((entry) => entry.run_id)).toEqual(["b"]);
    expect(waiting.length + working.length).toBe(runs.length);
  });

  test("l'ordine che arriva dal motore non si tocca", () => {
    // The engine orders from the oldest. Reordering here would mean two
    // ordering rules in two languages, and nobody would know which one wins.
    const runs = [
      run({ run_id: "vecchia", state: "waiting", since: 100 }),
      run({ run_id: "nuova", state: "waiting", since: 900 }),
    ];
    expect(groupRuns(runs).waiting.map((entry) => entry.run_id)).toEqual(["vecchia", "nuova"]);
  });
});
