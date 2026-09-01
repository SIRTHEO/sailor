/**
 * The step, open: what it was asked, what went in and came out, and what it is
 * saying right now. It appears only while a run holds this step — a panel with
 * empty boxes where no thread can fill them is worse than no panel.
 */
import { useContext, useMemo, useState } from "react";
import type { RunSnapshot } from "./engine";
import type { Graph, Step } from "./flow";
import { panesFromEvents } from "./RunConsole";
import { formatCost, formatTokens, usageIsPartial } from "./stepusage";
import { StepUsageContext } from "./StepNode";
import { mandateOf, neighboursOf } from "./stepfacts";

type Card = "mandate" | "inout" | "text";

const CARDS: { key: Card; label: string }[] = [
  { key: "mandate", label: "Mandate" },
  { key: "inout", label: "In & out" },
  { key: "text", label: "Text" },
];

function elapsed(from: number, to: number): string {
  const delta = Math.max(0, to - from);
  return `${String(Math.floor(delta / 60)).padStart(2, "0")}:${String(delta % 60).padStart(2, "0")}`;
}

interface StepLiveProps {
  step: Step;
  graph: Graph;
  run: RunSnapshot;
  /** Seconds, so an open step counts up without this component owning a clock. */
  now: number;
}

export function StepLive({ step, graph, run, now }: StepLiveProps) {
  const [card, setCard] = useState<Card>("mandate");
  const usage = useContext(StepUsageContext).get(step.id);
  const pane = useMemo(
    () => panesFromEvents(run.events).find((each) => each.stepId === step.id) ?? null,
    [run.events, step.id],
  );
  const neighbours = useMemo(() => neighboursOf(graph, step.id), [graph, step.id]);

  // The run has not reached this step: it has nothing to say about it, and
  // saying nothing is the honest answer.
  if (!pane) return null;

  const open = pane.endedAt === null;
  const mandate = mandateOf(step, pane.input);
  const said = pane.lines.filter((line) => line.stream !== "system");

  return (
    <section className="steplive">
      <header className="steplive__bar">
        <span className="steplive__state" data-outcome={pane.outcome ?? "open"}>
          {open ? "running" : (pane.outcome ?? "closed")}
        </span>
        <span className="steplive__time">{elapsed(pane.startedAt, pane.endedAt ?? now)}</span>
        {usage && (
          <span className="steplive__spend">
            {formatTokens(usage.inputTokens + usage.outputTokens)} tokens · {usage.calls} calls
            {usage.costMicros !== null && ` · ${formatCost(usage.costMicros)}${usageIsPartial(usage) ? " at least" : ""}`}
          </span>
        )}
      </header>

      <div className="steplive__around">
        <span className="steplive__side">
          before: {neighbours.before.length > 0 ? neighbours.before.join(", ") : "nothing — it starts the flow"}
        </span>
        <span className="steplive__side">
          after: {neighbours.after.length > 0 ? neighbours.after.join(", ") : "nothing — it ends here"}
        </span>
      </div>

      <nav className="steplive__cards">
        {CARDS.map((each) => (
          <button
            key={each.key}
            type="button"
            className="steplive__card"
            data-on={each.key === card || undefined}
            onClick={() => setCard(each.key)}
          >
            {each.label}
          </button>
        ))}
      </nav>

      {card === "mandate" && (
        <div className="steplive__body">
          {mandate !== null ? (
            <p className="steplive__mandate">{mandate}</p>
          ) : (
            <p className="steplive__none">this step was not given a mandate in words: it runs on its parameters</p>
          )}
        </div>
      )}

      {card === "inout" && (
        <div className="steplive__body">
          <h4 className="steplive__head">in</h4>
          <pre className="steplive__code">{JSON.stringify(pane.input ?? null, null, 2)}</pre>
          <h4 className="steplive__head">out</h4>
          {open ? (
            <p className="steplive__none">still running: nothing has come out yet</p>
          ) : (
            <pre className="steplive__code">{JSON.stringify(pane.output ?? null, null, 2)}</pre>
          )}
        </div>
      )}

      {card === "text" && (
        <div className="steplive__body">
          {said.length === 0 ? (
            <p className="steplive__none">
              {open ? "running, and it has not said anything yet" : "this step produced no text"}
            </p>
          ) : (
            <div className="steplive__said">
              {said.map((line) => (
                <div className="steplive__line" key={line.key} data-stream={line.stream}>
                  {line.text}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
