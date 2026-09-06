// What the board's head says: where its flows came from, and how the focused
// one last ran. **THE SUBJECT IS THE STAGE, NOT THE WINDOW.**

import type { RunSnapshot } from "./engine";
import { stepStatesOfRun } from "./runstate";

/** Where the flows on screen came from. */
export type Source = "loading" | "sample" | "engine" | "failed";

export interface BarStatus {
  live: boolean;
  word: string;
}

/**
 * How far a run has got, folded from its own facts: the snapshot carries no
 * counters and the ledger undercounts mid-run, so the denominator comes from
 * the flow on screen and the numerator from the events.
 */
export function runProgress(run: RunSnapshot): { done: number; running: number } {
  let done = 0;
  let running = 0;
  for (const step of stepStatesOfRun(run.events).values()) {
    if (step.state === "running") running += 1;
    else if (step.state !== "waiting") done += 1;
  }
  return { done, running };
}

/**
 * What the board's head may say about a run. The verdict of a check is not
 * here: `sailor flow check` has no door into this window, and borrowing a
 * verdict nobody gave is worse than showing none.
 */
export function statusOfRun(run: RunSnapshot | undefined, steps: number): BarStatus {
  if (run === undefined) return { live: false, word: "no run of this flow yet" };
  const { done, running } = runProgress(run);
  if (run.status === "running") {
    const at = Math.min(steps, done + (running > 0 ? 1 : 0));
    return { live: true, word: `a run in progress · step ${at} of ${steps}` };
  }
  return { live: false, word: `last run ${run.status} · ${done} of ${steps} steps closed` };
}

/**
 * Everything the head says about the flow in focus, as one value: passed apart,
 * each field carries its own chance of being drawn where no flow is — fault 120.
 */
export interface BarFlow {
  steps: number;
  dirty: boolean;
  busy: boolean;
  starting: boolean;
  status: BarStatus;
}

/**
 * Where the flows on screen came from, in words. A sample and a mute engine
 * name themselves: they are the two a person risks reading as true.
 */
export function sourceWords(source: Source, count: number, failure: string | null): string {
  if (source === "engine") return `${count} flows from the disk`;
  if (source === "failed") return `the engine is not answering: ${failure ?? "no reason given"}`;
  if (source === "loading") return "asking the engine for the flows…";
  return "sample data";
}
