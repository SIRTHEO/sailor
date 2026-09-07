// A scripted walkthrough for a person who has never opened Sailor: no
// repository, no engine, no profile. It plays a fake flow at a pace a human
// can follow, using the same marks and tokens the real terminal draws with.
//
// **NOTHING HERE TALKS TO AN ENGINE.** The steps and their output lines are
// written in this file, not fetched, not run. A demonstration that could fail
// for a real reason would stop being a safe first thing to click.

import { useEffect, useRef, useState } from "react";

export type DemoStepState = "queued" | "working" | "done";

export interface DemoStep {
  id: string;
  title: string;
  /** Lines revealed one at a time while the step is «working». */
  lines: string[];
}

/** ▬ working, ✓ done — the same shapes `TerminalPane` draws a real step with. */
const MARK: Record<DemoStepState, string> = { queued: "◇", working: "▬", done: "✓" };

/**
 * The script. An agent reads an errand, changes one file, runs the tests, and
 * opens a pull request — the shape of the one real activity this demo stands
 * in for, without touching anything real.
 */
export const DEMO_SCRIPT: DemoStep[] = [
  {
    id: "read-the-errand",
    title: "Reads what you asked for",
    lines: ["> reading the errand…", "found it: “the totals page rounds down instead of to the nearest cent”"],
  },
  {
    id: "make-the-change",
    title: "Changes one file",
    lines: ["> editing src/totals.ts", "- return Math.floor(cents) / 100;", "+ return Math.round(cents) / 100;"],
  },
  {
    id: "run-the-tests",
    title: "Runs the tests",
    lines: ["> npm test", "42 passed, 0 failed"],
  },
  {
    id: "open-the-pull-request",
    title: "Opens a pull request",
    lines: ["> gh pr create", "fix(totals): round to the nearest cent, not down"],
  },
];

/** How long a step stays «working» before the next one starts. */
export const STEP_MS = 1600;
/** How long between one revealed line and the next, within a step. */
export const LINE_MS = 520;

export interface DemoProps {
  open: boolean;
  onClose: () => void;
  /** Overridable only by tests: the real component always plays `DEMO_SCRIPT`. */
  script?: DemoStep[];
}

export function Demo({ open, onClose, script = DEMO_SCRIPT }: DemoProps) {
  const [playing, setPlaying] = useState(false);
  const [current, setCurrent] = useState(0);
  const [shownLines, setShownLines] = useState(0);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);

  /**
   * **CLOSING CANCELS THE SCRIPT, NOT ONLY THE VIEW.** Without this, a timer
   * from a first play would still be pending when a second one starts, and
   * would land its state update on top of it.
   */
  function clearTimers() {
    for (const timer of timers.current) clearTimeout(timer);
    timers.current = [];
  }

  useEffect(() => {
    if (!open) {
      clearTimers();
      setPlaying(false);
      setCurrent(0);
      setShownLines(0);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => clearTimers, []);

  function start() {
    clearTimers();
    setPlaying(true);
    setCurrent(0);
    setShownLines(0);
    scheduleStep(0);
  }

  function scheduleStep(index: number) {
    const step = script[index];
    if (step === undefined) return;
    for (let line = 1; line <= step.lines.length; line += 1) {
      timers.current.push(
        setTimeout(() => {
          setCurrent(index);
          setShownLines(line);
        }, line * LINE_MS),
      );
    }
    timers.current.push(
      setTimeout(() => {
        if (index + 1 < script.length) scheduleStep(index + 1);
        else setCurrent(script.length);
      }, STEP_MS),
    );
  }

  if (!open) return null;
  const finished = playing && current >= script.length;

  return (
    <div className="demo" role="dialog" aria-label="See Sailor at work">
      <button type="button" className="demo__close" onClick={onClose} aria-label="Close the demonstration">
        ×
      </button>
      <div className="demo__stage">
        <h1 className="demo__title">See Sailor at work</h1>
        <p className="demo__sub">
          A scripted walkthrough — no repository, no engine, nothing real is touched.
        </p>
        {!playing && (
          <button type="button" className="is-primary" onClick={start}>
            Start a demonstration
          </button>
        )}
        {playing && (
          <ol className="demo__steps">
            {script.map((step, index) => {
              const state: DemoStepState = index < current ? "done" : index === current ? "working" : "queued";
              return (
                <li key={step.id} className="demo__step" data-state={state}>
                  <span className="demo__mark" aria-hidden="true">
                    {MARK[state]}
                  </span>
                  <span className="demo__step-word">{state}</span>
                  <span className="demo__step-title">{step.title}</span>
                  {index === current && (
                    <pre className="demo__lines">{step.lines.slice(0, shownLines).join("\n")}</pre>
                  )}
                </li>
              );
            })}
          </ol>
        )}
        {finished && (
          <p className="demo__done">
            That is the whole of it — a scripted run, not a real one. Bring your own repository when you are ready.
          </p>
        )}
      </div>
    </div>
  );
}
