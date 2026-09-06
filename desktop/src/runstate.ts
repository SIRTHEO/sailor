import type { RunEvent, RunSnapshot } from "./engine";
import type { StepRun, StepState } from "./flow";
import { nodeId } from "./layout";

/**
 * From facts to per-node state.
 *
 * **WHY IT EXISTS.** The canvas said «waiting» on every node of every real flow,
 * even while the engine worked: the drawing got an empty map, and `executions` —
 * the real facts, which already arrive by event — was a dependency of nothing. The
 * sharpest possible breach of the permanent constraint «an interface that hides
 * what is happening is the opposite of the product»: it did not hide, it lied.
 *
 * **IT SITS OUTSIDE REACT** for the same reason as `linesFromEvents`: it is the
 * only part with a right answer and a wrong one, and a test can hand it events
 * and watch what it produces without mounting anything.
 */

/// The outcomes the store writes, translated into the states a node knows how
/// to draw. What is not in here stays `undefined` and the node falls back to
/// «waiting»: a state guessed on an outcome we do not know would be exactly the
/// lie this file exists to remove.
const STATE_OF_OUTCOME: Record<string, StepState> = {
  Went: "went",
  Broke: "broke",
  Stopped: "capped",
  Waiting: "waiting",
  // A step that answered «not yet, ask me on the next beat» is closed and
  // will be asked again: it waits, it is not still running.
  NotYet: "waiting",
  // A branch nobody took. Not waiting: nothing will make it run.
  Skipped: "skipped",
};

/**
 * The state of every step of **one** run, read from its events.
 *
 * A step that started and has not closed yet is running; one that closed carries
 * the outcome of the close. Events are read in `seq` order, not arrival order: an
 * attempt returning late must not rewrite the state of the one that overtook it.
 */
export function stepStatesOfRun(events: RunEvent[]): Map<string, StepRun> {
  const states = new Map<string, StepRun>();
  const startedAt = new Map<string, number>();
  const ordered = [...events].sort((a, b) => a.seq - b.seq);

  for (const event of ordered) {
    const stepId = event.step_id;
    if (stepId === null) continue;
    const payload = event.payload as Record<string, unknown> | null;

    if (event.kind === "step_started") {
      const attempt = typeof payload?.attempt === "number" ? payload.attempt : 1;
      const heldBy = typeof payload?.held_by_pid === "number" ? payload.held_by_pid : undefined;
      states.set(stepId, { step_id: stepId, state: "running", attempt, held_by_pid: heldBy });
      // WHEN THIS ATTEMPT BEGAN, kept aside rather than in the state: a retry
      // measured from the first start would report the wait between them too.
      startedAt.set(stepId, event.at);
      continue;
    }

    // WHAT IT IS SAYING WHILE IT RUNS. The engine has always sent this piece by
    // piece and nothing read it: on the canvas a step that had just printed
    // thirty lines and one stuck for eight minutes were drawn the same.
    if (event.kind === "step_text") {
      const current = states.get(stepId);
      if (current) states.set(stepId, { ...current, spoke_at: event.at });
      continue;
    }

    if (event.kind === "step_closed") {
      const outcome = typeof payload?.outcome === "string" ? payload.outcome : "";
      // THE SPECIES BEATS THE OUTCOME when it says the step is waiting on a
      // person: `hand_to_human` is a break nobody will retry, and the type
      // `StepState` keeps the two apart on purpose — showing it as any other
      // fault would send the watcher hunting a defect instead of answering.
      const species = typeof payload?.species === "string" ? payload.species : "";
      const previous = states.get(stepId);
      const began = startedAt.get(stepId);
      const state: StepState | undefined =
        outcome === "Broke" && species === "hand_to_human"
          ? "handed_to_human"
          : STATE_OF_OUTCOME[outcome];
      if (state === undefined) continue;
      states.set(stepId, {
        step_id: stepId,
        state,
        attempt: previous?.attempt ?? 1,
        // The two instants were in the events all along, and nothing read
        // them: the cell that shows this said «—» on every run there has
        // ever been.
        elapsed_secs: began === undefined ? previous?.elapsed_secs : event.at - began,
      });
    }
  }

  return states;
}

/**
 * The state of every node on the canvas, from all the runs known.
 *
 * **THE KEY IS THE QUALIFIED NAME `flusso::passo`, not the bare id.** On the one
 * canvas the flows sit together, and among the real ones on this machine three
 * ids are already repeated — `trigger`, `verifica`, `verdetto`. With the bare key
 * one flow's state would colour another's node of the same name, worse than the
 * grey it replaced: an error that reads as a measurement.
 *
 * When two runs of the same flow are known the most recent wins: it is the one
 * the watcher has just launched.
 */
export function stepStatesOfCanvas(runs: Iterable<RunSnapshot>): Map<string, StepRun> {
  const newest = new Map<string, RunSnapshot>();
  for (const run of runs) {
    const seen = newest.get(run.flow);
    if (!seen || run.started_at >= seen.started_at) newest.set(run.flow, run);
  }

  const states = new Map<string, StepRun>();
  for (const run of newest.values()) {
    for (const [stepId, state] of stepStatesOfRun(run.events)) {
      states.set(nodeId(run.flow, stepId), state);
    }
  }
  return states;
}
