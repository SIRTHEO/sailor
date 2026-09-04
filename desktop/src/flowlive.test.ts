import { describe, expect, test } from "vitest";
import type { RunEvent, RunSnapshot } from "./engine";
import { liveOf, newestPerFlow, STILL_SPEAKING_SECS } from "./flowlive";

/**
 * **THE COLUMN LISTED THIRTY-ONE NAMES AND A STEP COUNT.** Every fact below was
 * in the window already, and none of it was on the list where the flows are.
 */

function event(seq: number, kind: string, stepId: string | null, at: number, payload: unknown = {}): RunEvent {
  return { run_id: "r", seq, kind: kind as RunEvent["kind"], at, step_id: stepId, payload };
}

function run(over: Partial<RunSnapshot> = {}): RunSnapshot {
  return { run_id: "r", flow: "un-flusso", started_at: 100, status: "running", events: [], ...over };
}

describe("what is happening to a flow", () => {
  test("NOTHING KNOWN IS NOT AN EVENT: a flow nobody ran says nothing", () => {
    expect(liveOf(undefined, 1_000)).toBeNull();
  });

  test("A RUN GOING SAYS HOW FAR IT HAS GOT", () => {
    const live = liveOf(
      run({
        events: [
          event(1, "step_started", "uno", 100, { attempt: 1 }),
          event(2, "step_closed", "uno", 104, { outcome: "Went" }),
          event(3, "step_started", "due", 104, { attempt: 1 }),
        ],
      }),
      1_000,
    );
    expect(live).toEqual({ state: "running", done: 1, steps: 2, speaking: false });
  });

  test("AND WHETHER IT IS TALKING RIGHT NOW", () => {
    const events = [
      event(1, "step_started", "uno", 100, { attempt: 1 }),
      event(2, "step_text", "uno", 900, { text: "› reading\n" }),
    ];
    expect(liveOf(run({ events }), 900 + STILL_SPEAKING_SECS)).toMatchObject({ speaking: true });
    // Silence is nobody's event: it is read off the clock, and after it the row
    // goes back to saying only how far the run has got.
    expect(liveOf(run({ events }), 900 + STILL_SPEAKING_SECS + 1)).toMatchObject({ speaking: false });
  });

  test("A PERSON WAITING BEATS EVERYTHING ELSE, though the run is still «running»", () => {
    // The engine calls this run running, and drawn as such the row would ask
    // nothing while being the one thing on the screen that cannot go on.
    const live = liveOf(
      run({
        events: [
          event(1, "step_started", "decidi", 100, { attempt: 1 }),
          event(2, "step_closed", "decidi", 104, { outcome: "Broke", species: "hand_to_human" }),
          event(3, "step_started", "altro", 104, { attempt: 1 }),
          event(4, "step_text", "altro", 900, { text: "still going\n" }),
        ],
      }),
      900,
    );
    expect(live).toEqual({ state: "handed_to_human", waiting: 1 });
  });

  test("A RUN THAT ENDED SAYS HOW, and when it did", () => {
    const events = [
      event(1, "step_started", "uno", 100, { attempt: 1 }),
      event(2, "step_closed", "uno", 140, { outcome: "Broke" }),
    ];
    expect(liveOf(run({ status: "broke", events }), 1_000)).toEqual({ state: "broke", at: 140 });
    expect(liveOf(run({ status: "went", events }), 1_000)).toEqual({ state: "went", at: 140 });
  });

  test("AN ENDING NOBODY TAUGHT US IS NOT ONE WE INVENT", () => {
    // A status this window has no word for leaves the row as it was — a step
    // count — instead of a state guessed from a string.
    expect(liveOf(run({ status: "qualcosa-di-nuovo" }), 1_000)).toBeNull();
  });
});

describe("which run a row speaks for", () => {
  test("THE NEWEST, and newest is by when it started", () => {
    const rows = newestPerFlow([
      run({ run_id: "vecchia", flow: "uno", started_at: 100, status: "broke" }),
      run({ run_id: "nuova", flow: "uno", started_at: 900, status: "running" }),
      run({ run_id: "altra", flow: "due", started_at: 400, status: "went" }),
    ]);
    expect(rows.get("uno")?.run_id).toBe("nuova");
    expect(rows.get("due")?.run_id).toBe("altra");
  });

  test("NEWEST, NOT «THE ONE STILL RUNNING»: what happened to it is an answer too", () => {
    const rows = newestPerFlow([
      run({ run_id: "vecchia-viva", flow: "uno", started_at: 100, status: "running" }),
      run({ run_id: "recente-rotta", flow: "uno", started_at: 900, status: "broke" }),
    ]);
    expect(rows.get("uno")?.run_id).toBe("recente-rotta");
  });
});
