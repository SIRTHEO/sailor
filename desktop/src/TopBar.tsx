// The bar of the program: where the person is, what is in focus, and the two
// gestures that act on it.

import type { ReactNode } from "react";
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

interface TopBarProps {
  /** Where the person is: the section, and the entry inside it. */
  crumbs: string[];
  /** What runs, what it costs, who as: drawn from every place. */
  chips?: ReactNode;
  flowName: string | null;
  steps: number;
  dirty: boolean;
  busy: boolean;
  starting: boolean;
  source: Source;
  sourceWord: string;
  status: BarStatus | null;
  onWatch?: () => void;
  onSave: () => void;
  onRun: () => void;
}

/**
 * The bar of the program: the mark, the flow in focus, the three views of it,
 * and the two gestures that act on it.
 *
 * NO VERSION SITS NEXT TO THE NAME. The mockup draws a `v7` chip there; a flow
 * has no version — not in `flow::FlowFile`, not in the `.flow.json` on disk,
 * not in this window — and the Rust type refuses unknown fields, so one cannot
 * be added without changing the engine. The chip counts steps instead, which is
 * a number the flow really carries.
 */
export function TopBar({
  crumbs,
  chips,
  flowName,
  steps,
  dirty,
  busy,
  starting,
  source,
  sourceWord,
  status,
  onWatch,
  onSave,
  onRun,
}: TopBarProps) {
  const statusBody = status && (
    <>
      <span className="topbar__live" data-idle={status.live ? undefined : true} />
      <span className="topbar__status-word">{status.word}</span>
    </>
  );

  return (
    <header className="topbar">
      <span className="topbar__brand">
        <svg
          className="topbar__mark"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 17l9-13 9 13" />
          <path d="M3 17c2.5 2 5 2 7.5 0S16 15 18.5 17" />
        </svg>
        Sailor
      </span>
      <span className="topbar__rule" />

      <nav className="topbar__crumbs" aria-label="where you are">
        {crumbs.map((crumb, index) => (
          <span className="topbar__crumb" key={`${index}-${crumb}`}>
            {crumb}
          </span>
        ))}
      </nav>

      {/* NOTHING IS SAID ABOUT A FLOW WHERE THERE IS NONE. «No flow in focus —
          pick one in the rail» named a column six places have not got, and asked
          for a gesture the board now makes on its own. */}
      {flowName !== null && (
        <span className="topbar__flow">
          <span className="topbar__steps">{steps} steps</span>
          {dirty && (
            <span className="topbar__dirty">
              <span className="topbar__dot" />
              unsaved changes
            </span>
          )}
        </span>
      )}

      <span className="topbar__spacer" />

      {/* Whoever is looking must know whether these flows come from the disk or
          from a sample, without asking and without opening the code. */}
      <span className="topbar__source" data-source={source}>
        {sourceWord}
      </span>

      {status !== null &&
        (onWatch ? (
          <button type="button" className="topbar__status" onClick={onWatch}>
            {statusBody}
          </button>
        ) : (
          <span className="topbar__status">{statusBody}</span>
        ))}

      {chips}

      <button
        type="button"
        className="topbar__save"
        onClick={onSave}
        disabled={flowName === null || !dirty || busy}
      >
        {busy ? "Saving…" : "Save"}
      </button>
      {/* THE ACCENT MEANS «THE ACTION», and this is the action. Not a green:
          green is a step that went well, and prohibition 4 keeps the state
          colours for states. */}
      <button
        type="button"
        className="topbar__run is-primary"
        onClick={onRun}
        disabled={flowName === null || starting}
      >
        <span className="topbar__glyph" aria-hidden="true">
          ▶
        </span>
        {starting ? "Starting…" : "Run"}
      </button>
    </header>
  );
}
