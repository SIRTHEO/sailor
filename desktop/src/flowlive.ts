import type { RunSnapshot } from "./engine";
import { stepStatesOfRun } from "./runstate";

/**
 * What is happening to a flow, in one answer, for the row that names it. The
 * column listed thirty-one names and a step count: which one runs now, which
 * waits for you, which broke in the night was nowhere, though every fact was
 * already in the window. Outside React because it has a right answer.
 */

/** How long after a step's last word its flow is still «talking». */
export const STILL_SPEAKING_SECS = 2;

export type FlowLive =
  /** A run of it is going: how far it has got, and whether it is talking. */
  | { state: "running"; done: number; steps: number; speaking: boolean }
  /** It stopped on somebody: nothing will unblock it but a person. */
  | { state: "handed_to_human"; waiting: number }
  /** The last run ended, and how. `at` is when. */
  | { state: "went" | "broke" | "capped" | "stopped"; at: number };

/** What the ledger's words for an ending mean to a row. */
const ENDED_AS: Record<string, "went" | "broke" | "capped" | "stopped"> = {
  went: "went",
  ok: "went",
  broke: "broke",
  failed: "broke",
  capped: "capped",
  stopped: "stopped",
  halted: "stopped",
};

/**
 * The newest run of each flow. **Newest and not «the one still running»**: a
 * flow whose last run broke an hour ago says so until somebody runs it again,
 * which is the answer to «what happened to it», not «is it busy».
 */
export function newestPerFlow(runs: Iterable<RunSnapshot>): Map<string, RunSnapshot> {
  const newest = new Map<string, RunSnapshot>();
  for (const run of runs) {
    const seen = newest.get(run.flow);
    if (!seen || run.started_at >= seen.started_at) newest.set(run.flow, run);
  }
  return newest;
}

/**
 * **A PERSON WAITING BEATS EVERYTHING ELSE.** A run holding a handed step is
 * still «running» to the engine, and drawn as such it would ask nothing while
 * being the one thing on the screen that cannot go on without you.
 */
export function liveOf(run: RunSnapshot | undefined, now: number): FlowLive | null {
  if (!run) return null;
  const states = [...stepStatesOfRun(run.events).values()];

  const waiting = states.filter((step) => step.state === "handed_to_human").length;
  if (waiting > 0) return { state: "handed_to_human", waiting };

  if (run.status === "running") {
    const done = states.filter((step) => step.state !== "running").length;
    const speaking = states.some(
      (step) =>
        step.state === "running" &&
        step.spoke_at !== undefined &&
        now - step.spoke_at <= STILL_SPEAKING_SECS,
    );
    return { state: "running", done, steps: states.length, speaking };
  }

  // AN ENDING NOBODY TAUGHT US IS NOT AN ENDING WE INVENT. A status this table
  // has no word for leaves the row as it was: a step count, honestly.
  const ended = ENDED_AS[run.status];
  if (!ended) return null;
  const at = run.events.reduce((last, event) => Math.max(last, event.at), run.started_at);
  return { state: ended, at };
}
