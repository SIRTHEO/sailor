import { describe, expect, test } from "vitest";
import type { RunEvent, RunSnapshot } from "./engine";
import { stepStatesOfCanvas, stepStatesOfRun } from "./runstate";

/**
 * The first tests this window ever had, and they prove the only part with a
 * right answer and a wrong one: from events to per-node state.
 *
 * Each of them can come out otherwise, and each breaks when what it asserts is
 * broken: drop the ordering by `seq`, drop the qualified key, drop the
 * precedence of the species — and they turn red one by one.
 */

function started(seq: number, stepId: string, extra: Record<string, unknown> = {}): RunEvent {
  return { run_id: "r", seq, kind: "step_started", at: seq, step_id: stepId, payload: { attempt: 1, ...extra } };
}

function closed(seq: number, stepId: string, outcome: string, extra: Record<string, unknown> = {}): RunEvent {
  return { run_id: "r", seq, kind: "step_closed", at: seq, step_id: stepId, payload: { outcome, ...extra } };
}

function snapshot(flow: string, startedAt: number, events: RunEvent[]): RunSnapshot {
  return { run_id: `${flow}-${startedAt}`, flow, started_at: startedAt, status: "running", events };
}

describe("the state of a run", () => {
  test("a step started and not yet closed is running", () => {
    const states = stepStatesOfRun([started(1, "plan")]);
    expect(states.get("plan")?.state).toBe("running");
  });

  test("a closed step carries the outcome, no longer «in progress»", () => {
    const states = stepStatesOfRun([started(1, "plan"), closed(2, "plan", "Went")]);
    expect(states.get("plan")?.state).toBe("went");
  });

  test("the four endings stay distinct, because they are not interchangeable", () => {
    const states = stepStatesOfRun([
      closed(1, "a", "Went"),
      closed(2, "b", "Broke"),
      closed(3, "c", "Stopped"),
      closed(4, "d", "Broke", { species: "hand_to_human" }),
    ]);
    expect(states.get("a")?.state).toBe("went");
    expect(states.get("b")?.state).toBe("broke");
    expect(states.get("c")?.state).toBe("capped");
    // «Waiting for a person» is a break nobody will retry: showing it red sends
    // whoever looks hunting for a fault instead of answering.
    expect(states.get("d")?.state).toBe("handed_to_human");
  });

  test("an outcome we do not know leaves the step as it was, it does not guess", () => {
    const states = stepStatesOfRun([started(1, "x"), closed(2, "x", "Something_New")]);
    expect(states.get("x")?.state).toBe("running");
  });

  test("the events are read in seq order, not in arrival order", () => {
    // An attempt returning late must not rewrite the state of the one that
    // overtook it: here the close (seq 2) arrives AFTER the restart (seq 3).
    const states = stepStatesOfRun([started(1, "x"), started(3, "x", { attempt: 2 }), closed(2, "x", "Broke")]);
    expect(states.get("x")?.state).toBe("running");
    expect(states.get("x")?.attempt).toBe(2);
  });
});

describe("the state of the whole canvas", () => {
  test("the key is «flow::step»: two flows with the same id do not contaminate each other", () => {
    // It is the real case: among the flows on this machine `verifica`,
    // `trigger` and `verdetto` repeat. With the bare key one's state would
    // colour the other's namesake node — an error that reads as a measurement.
    const states = stepStatesOfCanvas([
      snapshot("first", 10, [closed(1, "verifica", "Went")]),
      snapshot("second", 11, [closed(1, "verifica", "Broke")]),
    ]);
    expect(states.get("first::verifica")?.state).toBe("went");
    expect(states.get("second::verifica")?.state).toBe("broke");
    expect(states.get("verifica")).toBeUndefined();
  });

  test("of two runs of the same flow the most recent counts", () => {
    const states = stepStatesOfCanvas([
      snapshot("f", 100, [closed(1, "p", "Broke")]),
      snapshot("f", 200, [started(1, "p")]),
    ]);
    expect(states.get("f::p")?.state).toBe("running");
  });

  test("with no runs no node has a state, and nobody invents one", () => {
    expect(stepStatesOfCanvas([]).size).toBe(0);
  });
});

/**
 * THE ONE NUMBER THE NODE HAD A SLOT FOR AND NEVER A VALUE. `elapsed_secs` was
 * carried forward and nothing ever assigned it, so the cell read `—` on every
 * run there has ever been — while both instants sat in the events.
 */
describe("how long a step took", () => {
  function at(seq: number, kind: string, stepId: string, at: number, payload: unknown = {}): RunEvent {
    return { run_id: "r", seq, kind: kind as RunEvent["kind"], at, step_id: stepId, payload };
  }

  test("it is the distance between the two instants the events carry", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1007, { outcome: "Went" }),
    ]);
    expect(states.get("read")?.elapsed_secs).toBe(7);
  });

  /* A step still running has no duration yet, and zero is not «no duration»:
     it is «it took no time», which is a different fact. */
  test("a step that has not closed has none", () => {
    const states = stepStatesOfRun([at(1, "step_started", "read", 1000, { attempt: 1 })]);
    expect(states.get("read")?.elapsed_secs).toBeUndefined();
  });

  /* A retry is a second run of the same step, and its duration is its own:
     measuring from the first attempt would report the wait between them too. */
  test("a retry is measured from its own start", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1002, { outcome: "Broke" }),
      at(3, "step_started", "read", 1060, { attempt: 2 }),
      at(4, "step_closed", "read", 1063, { outcome: "Went" }),
    ]);
    expect(states.get("read")?.elapsed_secs).toBe(3);
  });

  /* THE THIRD OUTCOME IS KNOWN TO THE WINDOW. A step that answered «not yet»
     is closed and will be asked again on the next beat: drawn as still
     running, it would look held by a process that is not there. */
  test("a step that said not yet is waiting, not running", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1001, { outcome: "NotYet" }),
    ]);
    expect(states.get("read")?.state).toBe("waiting");
  });
})

/**
 * **WHAT A STEP IS SAYING WHILE IT RUNS.** The engine has sent its output piece
 * by piece since the first run, and nothing read it: on the canvas a step that
 * had just printed thirty lines and one stuck for eight minutes were the same
 * node with the same breathing dot.
 */
describe("a step that is speaking", () => {
  function at(seq: number, kind: string, stepId: string, when: number, payload: unknown = {}): RunEvent {
    return { run_id: "r", seq, kind: kind as RunEvent["kind"], at: when, step_id: stepId, payload };
  }

  test("THE INSTANT OF ITS LAST PIECE OF OUTPUT IS KEPT, and it is the last one", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_text", "read", 1004, { pipe: "stdout", text: "› reading\n" }),
      at(3, "step_text", "read", 1009, { pipe: "stdout", text: "› done\n" }),
    ]);
    expect(states.get("read")?.spoke_at).toBe(1009);
    // And it is still running: speaking is something a running step does.
    expect(states.get("read")?.state).toBe("running");
  });

  test("A STEP THAT HAS SAID NOTHING HAS NO INSTANT, which is not «long ago»", () => {
    const states = stepStatesOfRun([at(1, "step_started", "read", 1000, { attempt: 1 })]);
    expect(states.get("read")?.spoke_at).toBeUndefined();
  });

  test("OUTPUT BEFORE THE START IS NOT AN ANSWER: there is no step to speak yet", () => {
    // Only what a started step says counts: a piece arriving for a step nobody
    // opened would invent a running step out of an ordering accident.
    const states = stepStatesOfRun([at(1, "step_text", "read", 1000, { text: "x" })]);
    expect(states.get("read")).toBeUndefined();
  });

  test("WHAT IT SAID DOES NOT SURVIVE THE CLOSE: a closed step is not speaking", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_text", "read", 1004, { text: "x" }),
      at(3, "step_closed", "read", 1006, { outcome: "Went" }),
    ]);
    expect(states.get("read")?.state).toBe("went");
    expect(states.get("read")?.spoke_at).toBeUndefined();
  });
});
