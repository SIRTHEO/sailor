// Question 2 of the three the window must answer: "what is running now, and
// what has it cost?" `WaitingScreen` already answers "what waits on me" and
// "what happened while I was away" — both about the past. Nothing until now
// named what is in flight, though the engine already carries it.
//
// Logic and view share this one file on purpose: a same-named sibling
// differing only by case (`RunningNow.tsx` / `runningnow.ts`) resolves to the
// wrong one on a case-insensitive volume — the component import came back
// `undefined` from the bundler picking the lowercase file first.

import { ago } from "./waiting";
import type { RunSnapshot } from "./engine";

/** The runs still going, newest first. Anything not `"running"` belongs to
 * the past screens, not this one. */
export function runningNow(runs: RunSnapshot[]): RunSnapshot[] {
  return runs
    .filter((run) => run.status === "running")
    .sort((a, b) => b.started_at - a.started_at);
}

export interface RunningRow {
  run: RunSnapshot;
  /** `null` while the cost has not been read yet, or could not be. */
  costMicros: number | null;
}

/** Dollars to two places, or the honest word for a cost not yet known —
 * never a silent zero standing in for "I have not asked". */
export function costWords(costMicros: number | null): string {
  if (costMicros === null) return "cost unknown";
  return `$${(costMicros / 1_000_000).toFixed(2)}`;
}

export function RunningNow({
  rows,
  now,
  onRun,
}: {
  rows: RunningRow[];
  now: number;
  onRun?: (runId: string) => void;
}) {
  if (rows.length === 0) return null;
  return (
    <section className="waiting__group">
      <h3 className="waiting__section">Running now</h3>
      {rows.map(({ run, costMicros }) => (
        <button
          key={run.run_id}
          type="button"
          className="waiting__row"
          onClick={() => onRun?.(run.run_id)}
        >
          <span className="waiting__text">{run.flow}</span>
          <span className="waiting__when">
            {ago(run.started_at, now)} · {costWords(costMicros)}
          </span>
        </button>
      ))}
    </section>
  );
}
