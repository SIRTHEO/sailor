import { useEffect, useMemo, useRef, useState } from "react";
import type { Ran, Refusal, RunEvent, RunSnapshot } from "./engine";
import { t as translate, tryT } from "./i18n";
import { totalsArePartial, type RunUsage } from "./flow";
import { StepRefusal } from "./StepRefusal";
import { StepRan } from "./StepRan";

/**
 * A run as it goes: what is running, what finished, what it said.
 *
 * **THE TEXT ARRIVES WHILE THE STEP IS STILL RUNNING.** `drain` in
 * `crates/actions/src/lib.rs` reads each pipe in a loop and hands every piece
 * to its sink, which the shell publishes as `step_text`: a line carries the
 * instant it really arrived. What the closing event brings is the whole buffer
 * again, plus what only it can carry — the answer of a step that declares a
 * shape, which never travelled as `stdout` at all.
 *
 * The front starts together — the cap is `AT_ONCE` in
 * `crates/flow/src/executor.rs`, four steps per wave.
 */

/** How a run is looked at. */
export type ConsoleMode = "inline" | "split";

/** One line of the view, with where it came from. */
export interface ConsoleLine {
  key: string;
  at: number;
  stepId: string | null;
  /** Where it comes from: the shell, the step's output, its errors, its saying. */
  stream: "system" | "stdout" | "stderr" | "said";
  text: string;
}

/**
 * The cap on the lines kept in view. An agent that talks for half an hour can
 * deliver tens of thousands of lines in one go: drawing them all freezes the
 * window exactly while the reader wants to read the ending. The **oldest** are
 * cut, and the cut is declared instead of making text vanish in silence.
 */
const MAX_LINES = 4000;

function splitText(text: string): string[] {
  return text.replace(/\n+$/, "").split("\n");
}

function pushText(
  lines: ConsoleLine[],
  seq: number,
  at: number,
  stepId: string | null,
  stream: ConsoleLine["stream"],
  text: unknown,
) {
  if (typeof text !== "string" || text.trim() === "") return;
  splitText(text).forEach((row, index) => {
    lines.push({ key: `${seq}:${stream}:${index}`, at, stepId, stream, text: row });
  });
}

/**
 * From facts to lines.
 *
 * It lives outside the component because it is the only part with a right and a
 * wrong answer: a test can hand it events and watch what it produces, without
 * mounting React.
 */
export function linesFromEvents(events: RunEvent[]): ConsoleLine[] {
  const lines: ConsoleLine[] = [];

  for (const event of events) {
    const payload = event.payload as Record<string, unknown> | null;
    switch (event.kind) {
      case "step_started": {
        lines.push({
          key: `${event.seq}:head`,
          at: event.at,
          stepId: event.step_id,
          stream: "system",
          text: `— «${event.step_id}» started`,
        });
        break;
      }
      case "step_closed": {
        const outcome = typeof payload?.outcome === "string" ? payload.outcome : "?";
        const failure = typeof payload?.failure_class === "string" ? payload.failure_class : null;
        lines.push({
          key: `${event.seq}:foot`,
          at: event.at,
          stepId: event.step_id,
          stream: failure ? "stderr" : "system",
          text: `— «${event.step_id}» closed: ${OUTCOME_LABEL[outcome] ?? outcome}${
            failure ? ` — ${whyFailed(failure)}` : ""
          }`,
        });
        // An external engine's output lives inside `output`; a shell check
        // keeps no text, and for that one there is nothing to show beyond the
        // outcome — which the pane declares.
        const output = payload?.output as Record<string, unknown> | null | undefined;
        if (output && typeof output === "object") {
          pushText(lines, event.seq, event.at, event.step_id, "stdout", output.stdout);
          pushText(lines, event.seq, event.at, event.step_id, "stderr", output.stderr);
          // **A STEP THAT DECLARES THE SHAPE OF ITS ANSWER GIVES NO MORE
          // `stdout`**: its output is `{status, answer}`, holding inside it only
          // the fields the shape declares. Reading `stdout` alone would leave
          // empty the pane of the steps that answer in shape — the very ones
          // most counted on. A trigger arrives here as an object too
          // (`{text, who, where, source, kind}`), and is read the same way.
          if (output.answer !== undefined && output.answer !== null) {
            pushText(
              lines,
              event.seq,
              event.at,
              event.step_id,
              "stdout",
              typeof output.answer === "string"
                ? output.answer
                : JSON.stringify(output.answer, null, 2),
            );
          }
        }
        // **A BROKEN STEP HAS NO `output`**: the useful text is all in `said`,
        // and the reason in `failure_class`. It is the case where the reader
        // most needs to read, and before this line it was the only one where
        // the view showed nothing.
        pushText(lines, event.seq, event.at, event.step_id, "said", payload?.said);
        break;
      }
      // WHAT A STEP SAYS WHILE IT RUNS. It arrives in pieces as the engine
      // writes them, under the pipe it came from: an error mixed into ordinary
      // output and indistinguishable from it is no more visible than silence.
      case "step_text": {
        const pipe = payload?.pipe === "err" ? "stderr" : "stdout";
        pushText(lines, event.seq, event.at, event.step_id, pipe, payload?.text);
        break;
      }
      case "stop_requested": {
        lines.push({
          key: `${event.seq}:stop`,
          at: event.at,
          stepId: null,
          stream: "system",
          text: "══ stop requested: no further step starts; the one running finishes",
        });
        break;
      }
      case "run_ended": {
        const status = typeof payload?.status === "string" ? payload.status : "?";
        const error = typeof payload?.error === "string" ? payload.error : null;
        lines.push({
          key: `${event.seq}:end`,
          at: event.at,
          stepId: null,
          stream: error ? "stderr" : "system",
          text: `══ the run ended: ${status}${error ? ` — ${error}` : ""}`,
        });
        // A run resumed through the window ends with the engine's own report:
        // what was reconciled, what was left to a person, the status it wrote.
        if (typeof payload?.report === "string" && payload.report !== "") {
          lines.push({ key: `${event.seq}:report`, at: event.at, stepId: null, stream: "system", text: payload.report });
        }
        break;
      }
      default: {
        pushText(lines, event.seq, event.at, event.step_id, "system", payload?.text);
      }
    }
  }

  if (lines.length <= MAX_LINES) return lines;
  const kept = lines.slice(lines.length - MAX_LINES);
  kept.unshift({
    key: "trimmed",
    at: kept[0]?.at ?? 0,
    stepId: null,
    stream: "system",
    text: `══ ${lines.length - MAX_LINES} older lines are not shown`,
  });
  return kept;
}

/** A step's state, for the side-by-side view. */
interface StepPane {
  stepId: string;
  startedAt: number;
  endedAt: number | null;
  outcome: string | null;
  failure: string | null;
  /** Which check refused, and what it saw, when the failure is a refusal. */
  refusal: Refusal | null;
  /** The program and the arguments the step started, when it started one. */
  ran: Ran | null;
  lines: ConsoleLine[];
  /** True if the step produced text of its own, beyond the system lines. */
  spoke: boolean;
  /** The step's action, to say whether it keeps text or an outcome alone. */
  action: string | null;
  /**
   * What entered the step.
   *
   * It already arrived inside `step_started` and was read **only** to guess the
   * action, then thrown away. Whoever watched a run saw what each step had said
   * and never what it had been given: half of the «clarity for the reader»
   * constraint was missing, and it was the half that explains the other.
   */
  input: unknown;
  /** What came out, kept whole: the lines made from it are not the thing. */
  output: unknown;
}

export function panesFromEvents(events: RunEvent[]): StepPane[] {
  const panes = new Map<string, StepPane>();
  const lines = linesFromEvents(events);

  for (const event of events) {
    if (!event.step_id) continue;
    const payload = event.payload as Record<string, unknown> | null;
    if (event.kind === "step_started") {
      panes.set(event.step_id, {
        stepId: event.step_id,
        startedAt: event.at,
        endedAt: null,
        outcome: null,
        failure: null,
        refusal: null,
        ran: null,
        lines: [],
        spoke: false,
        // The step record carries the input, from which what it runs is read.
        action: readAction(payload),
        input: payload?.input ?? null,
        output: null,
      });
    } else if (event.kind === "step_closed") {
      const pane = panes.get(event.step_id);
      if (pane) {
        pane.endedAt = event.at;
        pane.outcome = typeof payload?.outcome === "string" ? payload.outcome : null;
        pane.failure = typeof payload?.failure_class === "string" ? payload.failure_class : null;
        pane.refusal = readRefusal(payload?.refusal);
        pane.ran = readRan(payload?.ran);
        pane.output = payload?.output ?? null;
      }
    }
  }

  for (const line of lines) {
    if (!line.stepId) continue;
    const pane = panes.get(line.stepId);
    if (!pane) continue;
    pane.lines.push(line);
    if (line.stream !== "system") pane.spoke = true;
  }

  return Array.from(panes.values());
}

/** The refusal a closing fact carries, when it carries one whole. */
export function readRefusal(value: unknown): Refusal | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const { check, path, rule, seen } = record;
  if (typeof check !== "string" || typeof path !== "string" || typeof rule !== "string" || typeof seen !== "string") {
    return null;
  }
  return { check, path, rule, seen };
}

/** The line a closing fact carries, when it carries one whole. */
export function readRan(value: unknown): Ran | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const { program, args } = record;
  if (typeof program !== "string" || !Array.isArray(args)) return null;
  if (!args.every((word) => typeof word === "string")) return null;
  return { program, args: args as string[] };
}

/**
 * What a step runs, read from its record. A `command` is a shell check — which
 * keeps no text — a `bin` is an external engine, which keeps it. It is there to
 * tell the reader why a pane stays without lines.
 */
function readAction(payload: Record<string, unknown> | null): string | null {
  const input = payload?.input;
  if (!input || typeof input !== "object") return null;
  const record = input as Record<string, unknown>;
  // `source` is the field that a trigger alone declares; `tool` took `bin`'s
  // place when tools became identifiers instead of binary paths, and `bin` is
  // still read for the flows written before that.
  if (typeof record.source === "string") return "trigger";
  if (typeof record.tool === "string" || typeof record.bin === "string") return "external_engine";
  if (typeof record.command === "string") return "shell_check";
  return null;
}

/**
 * A failure class, in one readable line, from `run.failure.*`.
 *
 * **THE CLASSES ARE STABLE ENGINE NAMES, not free text**: they say *why* a step
 * fell without making anyone read the wall of output it produced. A class the
 * catalogue has never heard of shows as it came — an unknown name is
 * information, an invented sentence is not, which is why this asks `tryT`.
 */
export function whyFailed(failure: string): string {
  return tryT(`run.failure.${failure}`) ?? failure;
}

/** How a step ended, in a word a person reads. */
export const OUTCOME_LABEL: Record<string, string> = {
  Went: "went",
  Broke: "broke",
  Waiting: "waiting",
  /** The third outcome: it could not work now and asks to be asked again. */
  NotYet: "not yet",
  Stopped: "stopped",
  Skipped: "skipped",
};

function clock(at: number, since: number): string {
  const delta = Math.max(0, at - since);
  const minutes = Math.floor(delta / 60);
  const seconds = delta % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

interface RunConsoleProps {
  run: RunSnapshot;
  runs: RunSnapshot[];
  mode: ConsoleMode;
  /** The second it is now, so the counters of the open steps climb. */
  now: number;
  /**
   * Why the view is not listening, when it is not. A view that refreshes by
   * asking instead of listening stays true, but with a delay: the reader must
   * know, because it is the difference between «it has not happened yet» and
   * «I have not asked yet».
   */
  listenFailure: string | null;
  /**
   * What this run cost, when the ledger knows. `null` while it does not know
   * yet — and in that case nothing is shown, instead of showing zero.
   */
  usage: RunUsage | null;
  onMode: (mode: ConsoleMode) => void;
  onPick: (runId: string) => void;
  onClose: () => void;
  /** Asks the engine to stop this run before its next step. Rejects with the reason. */
  onStop: () => Promise<void>;
}

/** Whether a stop has been asked and the run has not ended yet. */
export function stopRequested(run: RunSnapshot): boolean {
  return run.status === "running" && run.events.some((event) => event.kind === "stop_requested");
}

/** Micro-units of currency as a person reads them: 128_541 → «$0.1285». */
function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(4)}`;
}

/** Thousands separated, the English way. */
function tokens(count: number): string {
  return count.toLocaleString("en-GB");
}

/**
 * The line of the spend.
 *
 * **WRITTEN CACHE IS SHOWN PLAINLY, APART FROM READ CACHE.** They are opposites:
 * reading costs a fraction of the input, writing costs more than the input. On
 * one measured call the write alone was 96% of the spend, with two input
 * tokens: putting them in the same box would hide the one item that counts.
 *
 * **AND A PARTIAL TOTAL SAYS SO.** If some call did not declare its own counts,
 * or had no price, the figure below is lower than the truth: keeping quiet
 * would be presenting a sum that hides what it lacks.
 */
export function costReading(usage: Pick<RunUsage, "tokens" | "total_cost_micros">): string {
  const { calls, calls_without_cost } = usage.tokens;
  if (calls === 0) return translate("ui.cost.nothing");
  if (calls_without_cost === calls) return translate("ui.cost.unknown", { calls });
  return translate(calls_without_cost > 0 ? "ui.cost.at_least" : "ui.cost.exact", {
    units: money(usage.total_cost_micros), calls, calls_without_cost,
  });
}

export function Spend({ usage }: { usage: RunUsage }) {
  const t = usage.tokens;
  if (t.calls === 0) return null;
  return (
    <div className="console__spend">
      <span className="console__spend-cost">{costReading(usage)}</span>
      <span>
        {t.calls} {t.calls === 1 ? "call" : "calls"}
      </span>
      <span>↑ {tokens(t.input_tokens)}</span>
      <span>↓ {tokens(t.output_tokens)}</span>
      {t.cached_tokens > 0 && <span title="read from the cache">cache read {tokens(t.cached_tokens)}</span>}
      {t.cache_write_tokens > 0 && (
        <span title="written to the cache: dearer than ordinary input">
          cache written {tokens(t.cache_write_tokens)}
        </span>
      )}
      {t.total_tokens_only > 0 && (
        <span title="engines that declare only the total, without the two sides">
          unsplit total {tokens(t.total_tokens_only)}
        </span>
      )}
      {totalsArePartial(t) && (
        <span className="console__spend-partial">
          partial total:{" "}
          {t.calls_without_tokens > 0 && `${t.calls_without_tokens} without counts`}
          {t.calls_without_tokens > 0 && t.calls_without_cost > 0 && ", "}
          {t.calls_without_cost > 0 && `${t.calls_without_cost} without a price`}
        </span>
      )}
    </div>
  );
}

export function RunConsole({
  run,
  runs,
  mode,
  now,
  listenFailure,
  usage,
  onMode,
  onPick,
  onClose,
  onStop,
}: RunConsoleProps) {
  const [stopTrouble, setStopTrouble] = useState<string | null>(null);
  const lines = useMemo(() => linesFromEvents(run.events), [run.events]);
  const panes = useMemo(() => panesFromEvents(run.events), [run.events]);
  const tail = useRef<HTMLDivElement | null>(null);

  // The tail stays in view as the run advances. It does not scroll if the
  // reader has moved up to read: yanking the view from under them is the way to
  // make unreadable the very line they were looking for.
  useEffect(() => {
    const box = tail.current;
    if (!box) return;
    const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
    if (nearBottom) box.scrollTop = box.scrollHeight;
  }, [lines.length, mode]);

  const running = run.status === "running";
  const stopping = stopRequested(run);
  const openPanes = panes.filter((pane) => pane.endedAt === null);

  return (
    <section className="console" aria-label="the run, as it goes">
      <header className="console__bar">
        <span className="console__title">Run</span>

        <select
          className="console__pick"
          value={run.run_id}
          aria-label="which run to watch"
          onChange={(event) => onPick(event.target.value)}
        >
          {runs.map((entry) => (
            <option key={entry.run_id} value={entry.run_id}>
              {entry.flow} · {entry.status}
            </option>
          ))}
        </select>

        <span className="console__status" data-status={run.status}>
          {stopping
            ? `stopping after the current step · ${clock(now, run.started_at)}`
            : running
              ? `running for ${clock(now, run.started_at)}`
              : run.status}
        </span>

        {/* THE STOP IS HONEST ABOUT WHAT IT CAN DO: the next step does not
            start; the one at work finishes, because the engine cannot take a
            step back from an agent already working on it. */}
        {running && !stopping && (
          <button
            type="button"
            className="console__stop"
            title="no further step starts; the one running finishes"
            onClick={() => {
              onStop().catch((error: unknown) => setStopTrouble(String(error)));
            }}
          >
            ■ Stop
          </button>
        )}
        {stopTrouble && <span className="console__trouble">{stopTrouble}</span>}

        <div className="console__spacer" />

        {/* The two ways of looking, at the reader's choice. */}
        <div className="console__modes" role="group" aria-label="how to look">
          <button type="button" data-on={mode === "inline" || undefined} onClick={() => onMode("inline")}>
            inline
          </button>
          <button type="button" data-on={mode === "split" || undefined} onClick={() => onMode("split")}>
            side by side
          </button>
        </div>

        <button type="button" className="console__close" onClick={onClose} title="close the view">
          ✕
        </button>
      </header>

      <div className="console__truth">
        {translate("window.session.execution_truth")}
      </div>

      {listenFailure && <div className="console__truth">{listenFailure}</div>}

      {usage && <Spend usage={usage} />}

      {mode === "inline" ? (
        <div className="console__lines" ref={tail}>
          {lines.length === 0 && <div className="console__empty">no lines, yet</div>}
          {lines.map((line) => (
            <div className="console__line" key={line.key} data-stream={line.stream}>
              <span className="console__time">{clock(line.at, run.started_at)}</span>
              {/* Who produced the line: in the inline view it is the one thing
                  that tells two mixed steps apart. */}
              <span className="console__who">{line.stepId ?? "run"}</span>
              <span className="console__text">{line.text}</span>
            </div>
          ))}
          {running && openPanes.length > 0 && (
            <div className="console__waiting">
              {openPanes
                .map((pane) => `«${pane.stepId}» running for ${clock(now, pane.startedAt)}`)
                .join(" · ")}
            </div>
          )}
        </div>
      ) : (
        <div className="console__panes" ref={tail}>
          {panes.length === 0 && <div className="console__empty">no steps, yet</div>}
          {panes.map((pane) => (
            <article className="pane" key={pane.stepId} data-open={pane.endedAt === null || undefined}>
              <header className="pane__bar">
                <span className="pane__id">{pane.stepId}</span>
                <span className="pane__state" data-outcome={pane.outcome ?? "open"}>
                  {pane.endedAt === null
                    ? `running · ${clock(now, pane.startedAt)}`
                    : `${OUTCOME_LABEL[pane.outcome ?? ""] ?? pane.outcome ?? "?"} · ${clock(
                        pane.endedAt,
                        pane.startedAt,
                      )}`}
                </span>
              </header>
              <div className="pane__body">
                {/* WHAT CAME IN, before what came out: it is the order in which
                    a step is understood, and until now there was the second
                    half alone. Closed by default — a long input would bury the
                    step's text, which stays the first thing looked at. */}
                {pane.input !== null && pane.input !== undefined && (
                  <details className="pane__input">
                    <summary className="pane__input-head">what came in</summary>
                    <pre className="pane__code">{JSON.stringify(pane.input, null, 2)}</pre>
                  </details>
                )}
                {/* WHAT IT ACTUALLY STARTED, between what came in and what it
                    said: the template is in the flow, the resolved line is the
                    only place a person can see which program really ran. */}
                {pane.ran && <StepRan ran={pane.ran} />}
                {/* «Lines» is not «the step's text»: the system lines —
                    started, closed — are always there, and counting them as
                    text would make the note vanish in the very panes that need
                    it, the ones where the step said nothing. */}
                {/* A running step that has said nothing has said nothing yet:
                    its text arrives as the engine writes it, not at the end. */}
                {!pane.spoke && pane.endedAt === null && (
                  <div className="pane__note">running, and it has not said anything yet</div>
                )}
                {!pane.spoke && pane.endedAt !== null && (
                  <div className="pane__note">
                    {pane.action === "shell_check"
                      ? "a shell check keeps no text of its own: of this step the outcome remains"
                      : "this step produced no text"}
                  </div>
                )}
                {pane.lines.map((line) => (
                  <div className="pane__line" key={line.key} data-stream={line.stream}>
                    {line.text}
                  </div>
                ))}
                {pane.refusal && <StepRefusal refusal={pane.refusal} />}
                {pane.failure && <div className="pane__failure">{whyFailed(pane.failure)}</div>}
              </div>
            </article>
          ))}
        </div>
      )}

      {/* No warning that two panes advance at once: the front really does start
          together — two six-second steps measured at 6.07 — so two panes moving
          together tell the truth. */}
    </section>
  );
}
