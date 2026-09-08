// The first screen: what is happening now.
//
// NOT AN INVENTORY OF FLOWS. Opening on the canvas answers «what could I run»,
// while whoever reopens the window is asking whether something is still
// working, something broke, or something is waiting on them.
//
// The question is asked directly: `open_runs` asks the store what is open.
// There is no historical list to filter, because that is not the question — in
// fifteen products compared, every one of them filtered a history by status.
//
// «WAITING ON YOU» IS A STATE, NOT A DOT. It is a named group, at the top, and
// the word sits beside the tint: prohibition 5 does not let a colour carry a
// state on its own.

import { Fragment } from "react";
import { useAsk, useClock } from "./ask";
import { openRuns, todaySummary, type DaySummary, type OpenRun } from "./engine";
import { Handed } from "./Handed";

/** How often the list is asked of the ledger again. */
const REFRESH_MS = 4000;

/** Today's summary changes more slowly: it sums whole runs. */
const SUMMARY_MS = 30000;

/** A number with thousand separators, as a person reads it. */
function count(value: number): string {
  return value.toLocaleString("en-GB");
}

/** From micro-units to dollars with three decimals: below a thousandth nothing is decided. */
function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(3)}`;
}

/**
 * Today's summary.
 *
 * **IT ALSO SAYS WHAT IT COULD NOT MEASURE.** It is the line missing from the
 * other products, and the survey found why it counts: Langfuse showed 4,509
 * tokens where there were 2,265, LangSmith inflates by 75-200 times with images
 * and does not count the prompt cache, and someone at Arize says of Phoenix
 * that «the cost is computed correctly in the database, but hard to work out
 * from the UI». A number shown with authority and wrong is worse than an absent
 * number. Here, if some call carried no tokens or price, the figure reads
 * beside the number of calls that do not make it up.
 */
function Today({ summary }: { summary: DaySummary }) {
  if (!summary.ledger_present) {
    return (
      <p className="now__mute">
        The ledger does not exist yet: nothing has ever run on this machine. No count is known, which is not
        the same as «zero».
      </p>
    );
  }
  const seen = summary.input_tokens + summary.output_tokens + summary.cached_tokens + summary.cache_write_tokens;
  return (
    <section className="today">
      <span className="today__label">Today</span>
      <span className="today__cell">
        <b>{count(summary.runs)}</b> runs
      </span>
      <span className="today__cell">
        <b>{count(summary.went)}</b> went
      </span>
      {summary.broke > 0 && (
        <span className="today__cell" data-gravity="danger">
          <b>{count(summary.broke)}</b> broke
        </span>
      )}
      {summary.still_open > 0 && (
        <span className="today__cell">
          <b>{count(summary.still_open)}</b> still open
        </span>
      )}
      <span className="today__cell">
        <b>{count(seen)}</b> token
      </span>
      <span className="today__cell">
        <b>{money(summary.cost_micros)}</b>
      </span>
      {(summary.unmeasured > 0 || summary.unpriced > 0) && (
        <span className="today__caveat" data-gravity="warn">
          {summary.unmeasured > 0 && `${count(summary.unmeasured)} calls without tokens`}
          {summary.unmeasured > 0 && summary.unpriced > 0 && " · "}
          {summary.unpriced > 0 && `${count(summary.unpriced)} without a price`}
          {" — the figures above do not contain them"}
        </span>
      )}
    </section>
  );
}

/**
 * How long it has lasted, said as a person would say it.
 *
 * **IT ROUNDS DOWN, ALWAYS.** «2 h» on a run stopped for two hours and fifty
 * minutes is less grave than «3 h» on one stopped for two and ten: the reader
 * decides whether to step in, and an inflated number makes them step in on
 * something that is not a problem yet.
 */
export function howLong(seconds: number): string {
  if (seconds < 0) return "—";
  if (seconds < 60) return `${Math.floor(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ${minutes % 60} min`;
  const days = Math.floor(hours / 24);
  return `${days} d ${hours % 24} h`;
}

/**
 * What to say about the process holding an open step.
 *
 * «gone» and «unknown» are not the same news: the first is a step nothing will
 * ever close, the second is a step the ledger recorded without a pid, and only
 * the first is a reason to step in.
 */
export function holderWord(holder: "alive" | "gone" | "unknown"): string {
  return holder === "gone" ? "nobody there" : "holder unknown";
}

/**
 * The runs split into the two groups, each oldest first.
 *
 * The order inside a group already comes from the engine; here they are merely
 * separated, and the separation is the point: a run waiting on a person and one
 * at work do not queue together, because one of the two asks something of the
 * reader and the other does not.
 */
export function groupRuns(runs: OpenRun[]): { waiting: OpenRun[]; working: OpenRun[] } {
  return {
    waiting: runs.filter((run) => run.state === "waiting"),
    working: runs.filter((run) => run.state === "working"),
  };
}

interface NowProps {
  /** True inside the native shell: outside there is no ledger to ask. */
  native: boolean;
  /** Open the run on the canvas, to look inside it. */
  onOpen: (runId: string) => void;
}

export function Now({ native, onOpen }: NowProps) {
  const outside = "outside the shell: the engine reads the ledger";
  const { asked } = useAsk<OpenRun[]>(native, openRuns, REFRESH_MS, outside);
  const { asked: day } = useAsk<DaySummary>(native, todaySummary, SUMMARY_MS, outside);
  const now = useClock();

  if (asked.state === "mute") {
    // **A MUTE LEDGER IS SAID SO.** An empty screen and an unreachable ledger
    // look far too alike, and the second is the one where work goes on in the
    // belief that nothing is running.
    return (
      <div className="now">
        <p className="now__mute">Cannot ask what is running: {asked.why}</p>
      </div>
    );
  }

  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Asking the ledger what is open…</p>
      </div>
    );
  }

  const { waiting, working } = groupRuns(asked.value);

  return (
    <div className="now">
      {day.state === "answered" && <Today summary={day.value} />}
      {asked.value.length === 0 && (
        <p className="now__empty">Nothing is running, and nothing waits for you.</p>
      )}
      {waiting.length > 0 && (
        <RunGroup
          title="Waiting for you"
          note="still until you do something"
          runs={waiting}
          now={now}
          onOpen={onOpen}
        />
      )}
      {working.length > 0 && (
        <RunGroup title="At work" note="somebody or something is working on them" runs={working} now={now} onOpen={onOpen} />
      )}
    </div>
  );
}

/**
 * A group of runs, drawn without asking anyone anything.
 *
 * **IT IS SEPARATE FROM `Now` SO IT CAN BE MEASURED.** `Now` asks the ledger,
 * and outside the native shell it has nobody to ask: a test that drew `Now`
 * would measure the sentence «I cannot ask» and believe it had looked at the
 * screen. The contrast check draws this.
 */
interface GroupProps {
  title: string;
  note: string;
  runs: OpenRun[];
  now: number;
  onOpen: (runId: string) => void;
}

export function RunGroup({ title, note, runs, now, onOpen }: GroupProps) {
  return (
    <section className="now__group">
      <header className="now__head">
        <h2 className="now__title">{title}</h2>
        <span className="now__count">{runs.length}</span>
        <span className="now__note">{note}</span>
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>run</th>
            <th>state</th>
            <th className="now__num">for</th>
            <th>what it is doing</th>
            <th>started</th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <Fragment key={run.run_id}>
            <tr data-followable={run.started_here || undefined}>
              <td className="now__entity">
                {run.entity === "" ? <span className="now__unnamed">unnamed</span> : run.entity}
                <span className="now__id">{run.run_id}</span>
              </td>
              {/* The word carries the state as much as the tint: prohibition 5. */}
              <td className="now__state" data-state={run.state}>
                {run.state === "waiting" ? "waits for you" : "working"}
              </td>
              <td className="now__num">{howLong(now - run.since)}</td>
              {/* WHICH STEP, NOT HOW MANY. «3 steps open» says nothing: a step
                  open for six minutes is working, the same one open for three
                  hours is hung. The attempt is written when it is not the first
                  — «2nd time» on an open step means the first round fell, and
                  that is the information a green row hides. */}
              <td className="now__steps">
                {run.state === "waiting" ? (
                  "—"
                ) : run.open_now.length === 0 ? (
                  run.open_steps
                ) : (
                  run.open_now.map((step) => (
                    <span className="now__step" key={`${step.step_id}::${String(step.attempt)}`}>
                      {step.step_id}
                      {step.attempt > 1 && <b>attempt {step.attempt}</b>}
                      <i>{howLong(step.open_for_secs)}</i>
                      {step.holder !== "alive" && <u>{holderWord(step.holder)}</u>}
                    </span>
                  ))
                )}
              </td>
              {/* ONLY WHAT CAN REALLY BE OPENED IS OPENED. A run's live text
                  lives in the memory of the shell that started it: a run begun
                  from the terminal shows here — the whole point of this screen
                  — but cannot be followed yet. A button that opens nothing is
                  worse than no button: whoever presses it concludes the window
                  is broken. Here the row says why, instead of pretending. */}
              <td className="now__from">
                {run.started_here ? (
                  <button type="button" className="now__open" onClick={() => onOpen(run.run_id)}>
                    watch
                  </button>
                ) : (
                  <span className="now__elsewhere">started elsewhere</span>
                )}
              </td>
            </tr>
            {/* WHAT IT WAITS FOR, AND THE GESTURE THAT ANSWERS IT, under the
                row: a run parked on a person is worth nothing as a line alone. */}
            {run.state === "waiting" && (
              <tr className="now__handed">
                <td colSpan={5}>
                  <Handed runId={run.run_id} />
                </td>
              </tr>
            )}
            </Fragment>
          ))}
        </tbody>
      </table>
    </section>
  );
}
