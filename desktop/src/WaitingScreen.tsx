// The screen the window opens on: the decisions that wait for a person, and
// what happened unattended. Those two, and nothing else.
//
// **NO GESTURE IS INVENTED HERE.** Taking a handed step and closing it are
// `Handed`'s, and a copy would be a second report of one act; opening a run is
// navigation, which whoever wires the window owns.

import { useEffect, useState, type ReactNode } from "react";
import {
  AWAY_HOURS,
  HOW_MANY_SOURCES,
  STATE_WORD,
  ago,
  blindSpots,
  abandonedDecisions,
  brokenDecisions,
  handedDecisions,
  headingOf,
  inOrder,
  quotaDecisions,
  readSources,
  reportsFrom,
  spanOf,
  spent,
  subOf,
  unknownStatuses,
  waitedFor,
  type Decision,
  type Report,
  type RowState,
  type Sources,
} from "./waiting";

/** How often it is reread. A morning list that never moves is a photograph. */
const REFRESH_MS = 10000;

const OUTSIDE = "outside the desktop shell there is no engine to ask";

/**
 * The state as a shape, one per state and none of them alike. Colour alone
 * fails whoever cannot see it, and the sheet forbids it outright: the word
 * from `STATE_WORD` sits beside the mark, always.
 */
const MARK: Record<RowState, ReactNode> = {
  human: <path d="M12 3l9 9-9 9-9-9z" fill="currentColor" stroke="none" />,
  broke: <path d="M6 6l12 12M18 6L6 18" />,
  quota: <path d="M12 4l9 16H3z" />,
  went: <path d="M4 12.5l5 5L20 6.5" />,
  capped: <path d="M4 6h16M12 6v12" />,
  stopped: <path d="M6 6h12v12H6z" />,
};

function Mark({ state }: { state: RowState }) {
  return (
    <span className="waiting__mark" data-state={state}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true">
        {MARK[state]}
      </svg>
    </span>
  );
}

interface RowProps {
  state: RowState;
  what: string;
  context: string;
  /** The right-hand column: how long it waited, or what it cost and when. */
  when: string;
  pale?: boolean;
  children?: ReactNode;
}

function Row({ state, what, context, when, pale, children }: RowProps) {
  return (
    <article className={pale ? "waiting__row waiting__row--pale" : "waiting__row"}>
      <Mark state={state} />
      <div className="waiting__text">
        <b className="waiting__what">{what}</b>
        <span className="waiting__context">
          <span className="waiting__word">{STATE_WORD[state]}</span> · {context}
        </span>
      </div>
      <span className="waiting__when">{when}</span>
      <div className="waiting__acts">{children}</div>
    </article>
  );
}

export interface WaitingScreenProps {
  native: boolean;
  now: number;
  /** From when «while you were away» counts, in seconds from the epoch. */
  since?: number;
  /** Given by a test, or by whoever already holds the answers. */
  sources?: Sources;
  /** Absent, the row draws no gesture rather than one that goes nowhere. */
  onRun?: (runId: string) => void;
  onQuota?: () => void;
}

export function WaitingScreen({ native, now, since, sources, onRun, onQuota }: WaitingScreenProps) {
  const [own, setOwn] = useState<Sources | null>(null);
  const from = since ?? now - AWAY_HOURS * 3600;

  useEffect(() => {
    if (!native || sources !== undefined) return;
    let watching = true;
    const read = () => {
      readSources().then((seen) => {
        if (watching) setOwn(seen);
      });
    };
    read();
    const tick = window.setInterval(read, REFRESH_MS);
    return () => {
      watching = false;
      window.clearInterval(tick);
    };
  }, [native, sources !== undefined]);

  const seen = sources ?? own;
  if (seen === null) {
    // AN ENGINE THAT IS NOT THERE IS NOT AN EMPTY MORNING, and the two would
    // otherwise draw the same blank screen.
    if (!native) return <p className="waiting__note" data-bad>I cannot tell you what waits: {OUTSIDE}</p>;
    return <p className="waiting__note">Asking what waits…</p>;
  }

  const blind = blindSpots(seen);
  if (blind.length === HOW_MANY_SOURCES) {
    return (
      <p className="waiting__note" data-bad>
        I cannot tell you what waits. Nothing here answered: {blind.join("; ")}
      </p>
    );
  }

  const decisions = inOrder([
    ...(seen.open.state === "answered" && seen.handed.state === "answered"
      ? handedDecisions(seen.open.value, seen.handed.value)
      : []),
    ...(seen.history.state === "answered" ? brokenDecisions(seen.history.value, from) : []),
    ...(seen.quota.state === "answered" ? quotaDecisions(seen.quota.value, now) : []),
    ...abandonedDecisions(seen.terminals),
  ]);
  const history = seen.history.state === "answered" ? seen.history.value : null;
  const reports = history === null ? [] : reportsFrom(history, from);
  const unknown = history === null ? [] : unknownStatuses(history, from);

  return (
    <div className="waiting">
      <header className="waiting__head">
        <h2 className="waiting__title">{headingOf(decisions.length, blind.length > 0)}</h2>
        <p className="waiting__sub">{subOf(reports.length, history === null)}</p>
      </header>

      {blind.length > 0 && (
        <p className="waiting__blind" data-bad>
          This list is short of whatever these hold, and I cannot say how much: {blind.join("; ")}
        </p>
      )}

      {decisions.length > 0 && (
        <section className="waiting__group">
          <h3 className="waiting__section">Decide</h3>
          {decisions.map((one) => (
            <DecisionRow key={one.id} decision={one} now={now} onRun={onRun} onQuota={onQuota} />
          ))}
        </section>
      )}

      {reports.length > 0 && (
        <section className="waiting__group">
          <h3 className="waiting__section">
            While you were away <span className="waiting__span">{spanOf(from, now)}</span>
          </h3>
          {reports.map((one) => (
            <ReportRow key={one.id} report={one} now={now} onRun={onRun} />
          ))}
        </section>
      )}

      {unknown.length > 0 && (
        <p className="waiting__note" data-bad>
          {unknown.length === 1 ? "One run ended" : `${String(unknown.length)} runs ended`} under a
          status this window does not know, and is filed nowhere above: {unknown.join(", ")}
        </p>
      )}
    </div>
  );
}

function DecisionRow({
  decision,
  now,
  onRun,
  onQuota,
}: {
  decision: Decision;
  now: number;
  onRun?: (runId: string) => void;
  onQuota?: () => void;
}) {
  const runId = decision.runId;
  return (
    <Row
      state={decision.state}
      what={decision.question}
      context={decision.context}
      when={decision.since === null ? "" : waitedFor(decision.since, now)}
    >
      {runId !== null && onRun !== undefined && (
        <button type="button" className="waiting__act waiting__act--primary" onClick={() => onRun(runId)}>
          Open the run
        </button>
      )}
      {decision.state === "quota" && onQuota !== undefined && (
        <button type="button" className="waiting__act waiting__act--primary" onClick={onQuota}>
          Open the quota
        </button>
      )}
    </Row>
  );
}

function ReportRow({
  report,
  now,
  onRun,
}: {
  report: Report;
  now: number;
  onRun?: (runId: string) => void;
}) {
  const cost = spent(report.costMicros);
  return (
    <Row
      state={report.state}
      what={report.what}
      context={report.context}
      when={(cost === null ? "" : `${cost} · `) + ago(report.at, now)}
      pale
    >
      {onRun !== undefined && (
        <button type="button" className="waiting__act" onClick={() => onRun(report.runId)}>
          Why →
        </button>
      )}
    </Row>
  );
}
