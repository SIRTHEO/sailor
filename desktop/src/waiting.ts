// What waits for a person, and what happened while the window was shut.
//
// **EVERY ROW HERE NAMES A COMMAND THE SHELL ANSWERS**, the census of the
// machine's terminals included, since `terminals_abandoned` exposes it. A fact
// nothing answers for stays absent rather than guessed from the panes this
// window happens to hold open, which is a different thing.

import type { Asked } from "./ask";
import {
  executionHistory,
  handedSteps,
  openRuns,
  type Execution,
  type HandedStep,
  type OpenRun,
} from "./engine";
import { quota, windowName, type Quota, type Window as QuotaWindow } from "./quota";
import { abandonedTerminals, type Abandoned } from "./terminal";

/** The state a row carries. Drawn as a shape, and said as a word beside it. */
export type RowState = "human" | "broke" | "quota" | "went" | "capped" | "stopped";

export const STATE_WORD: Record<RowState, string> = {
  human: "waiting on you",
  broke: "broke",
  quota: "near its limit",
  went: "went",
  capped: "at its cap",
  stopped: "stopped",
};

/**
 * The order on screen. **A RANK IS HOW MUCH STAYS HELD UP WHILE YOU DO NOT
 * ANSWER**: a step handed to a person freezes a whole run and only you can
 * free it; a run that broke has already stopped and leaving it costs nothing
 * more; a quota near its limit blocks nothing until the next run asks.
 */
export const RANK: Record<RowState, number> = {
  human: 0,
  broke: 1,
  quota: 2,
  went: 3,
  capped: 3,
  stopped: 3,
};

/** A thing that does not move until a person answers it. */
export interface Decision {
  id: string;
  state: RowState;
  question: string;
  context: string;
  /** When it began waiting; `null` when nothing in the fact dates it. */
  since: number | null;
  runId: string | null;
  stepId: string | null;
}

/** Something that happened unattended. Nothing is asked of it. */
export interface Report {
  id: string;
  state: RowState;
  what: string;
  context: string;
  at: number;
  costMicros: number;
  runId: string;
}

/** How far back «while you were away» reaches when nobody says otherwise. */
export const AWAY_HOURS = 12;

/** A window this far spent is the next run's refusal, not this one's. */
export const NEAR_ITS_LIMIT = 0.9;

export function awaySince(now: number, hours: number = AWAY_HOURS): number {
  return now - hours * 3600;
}

/** The line a person answers: a mandate is a paragraph, a row holds one line. */
export function firstLine(text: string): string {
  return (
    text
      .split("\n")
      .map((line) => line.trim())
      .find((line) => line !== "") ?? ""
  );
}

/** How long ago, in the words a person uses. */
export function ago(at: number, now: number): string {
  const gap = Math.max(0, now - at);
  if (gap < 60) return "just now";
  if (gap < 3600) return `${String(Math.floor(gap / 60))}m ago`;
  if (gap < 86_400) return `${String(Math.floor(gap / 3600))}h ago`;
  return `${String(Math.floor(gap / 86_400))}d ago`;
}

/** How long it has been waiting. The one number that shames a screen. */
export function waitedFor(since: number, now: number): string {
  const gap = Math.max(0, now - since);
  const minutes = Math.floor(gap / 60);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `for ${String(minutes)}m`;
  return `for ${String(Math.floor(minutes / 60))}h ${String(minutes % 60)}m`;
}

/** What a run cost, or nothing at all: zero micros is «it called no model». */
export function spent(micros: number): string | null {
  return micros === 0 ? null : `$${(micros / 1_000_000).toFixed(2)}`;
}

/**
 * How a finished run ended, in the words the engine writes. **THE CAP IS READ
 * FIRST**: a run stopped by its own ceiling also carries an `error`, and
 * reading that first would file a bounded stop among the faults. `null` is a
 * status this version has never heard of, and it is not filed under a word.
 */
export function outcomeOfRun(run: Execution): RowState | null {
  if (run.status === "cap_reached") return "capped";
  if (run.status === "failed" || run.steps_broke > 0 || run.error !== null) return "broke";
  if (run.status === "complete" || run.status === "succeeded") return "went";
  if (run.status === "stopped") return "stopped";
  return null;
}

/** The runs that ended in the span looked at. */
function endedSince(history: Execution[], since: number): Execution[] {
  return history.filter((run) => run.ended_at !== null && run.ended_at >= since);
}

/** Statuses this version cannot read, so the screen can say it saw them. */
export function unknownStatuses(history: Execution[], since: number): string[] {
  const seen = new Set<string>();
  for (const run of endedSince(history, since)) {
    if (outcomeOfRun(run) === null) seen.add(run.status);
  }
  return [...seen].sort();
}

/** The steps of the stopped runs, each as the one question it asks. */
export function handedDecisions(
  open: OpenRun[],
  handed: Record<string, HandedStep[]>,
): Decision[] {
  const decisions: Decision[] = [];
  for (const run of open) {
    if (run.state !== "waiting") continue;
    for (const step of handed[run.run_id] ?? []) {
      decisions.push({
        id: `${run.run_id}/${step.step_id}`,
        state: "human",
        question: firstLine(step.mandate) || "A step waits for a person and states no mandate.",
        context:
          `${run.entity} · ${step.step_id}` +
          (step.holder === "" ? "" : ` · offered to ${step.holder}`),
        since: step.since,
        runId: run.run_id,
        stepId: step.step_id,
      });
    }
  }
  return decisions;
}

/**
 * Terminals the register calls open that hold nobody. A terminal killed
 * without closing stays open for ever, by design; nothing dates its death, so
 * the row carries no «since» rather than an invented one.
 */
export function abandonedDecisions(asked: Asked<Abandoned>): Decision[] {
  if (asked.state !== "answered" || asked.value.answer !== "seen") return [];
  return asked.value.ttys.map((tty) => ({
    id: `terminal/${tty}`,
    state: "stopped" as const,
    question: `«${tty}» is open on the register and holds nobody`,
    context: "killed without closing: nothing runs there any more",
    since: null,
    runId: null,
    stepId: null,
  }));
}

/** The runs that broke in the span looked at. */
export function brokenDecisions(history: Execution[], since: number): Decision[] {
  const broken = endedSince(history, since).filter((run) => outcomeOfRun(run) === "broke");
  return broken.map((run) => ({
    id: run.run_id,
    state: "broke" as const,
    question: `${run.entity} broke`,
    context:
      `${String(run.steps_went)} of ${String(run.steps_total)} steps went` +
      (run.error === null ? "" : ` · ${firstLine(run.error)}`),
    since: run.ended_at,
    runId: run.run_id,
    stepId: null,
  }));
}

/** The windows near their limit, **and the accounts that did not answer**: one
 * nobody can read answers nothing, which reads exactly like one with room. */
export function quotaDecisions(
  read: Quota,
  now: number,
  near: number = NEAR_ITS_LIMIT,
): Decision[] {
  const out: Decision[] = read.unreachable.map((one) => ({
    id: `unreachable/${one.account}`,
    state: "quota" as const,
    question: `${one.account} did not answer, so nothing is known about its quota`,
    context: one.why,
    since: null,
    runId: null,
    stepId: null,
  }));
  return out.concat(near_the_limit(read.windows, now, near));
}

function near_the_limit(windows: QuotaWindow[], now: number, near: number): Decision[] {
  return windows
    .filter((one) => one.spent_fraction >= near)
    .map((one) => ({
      id: `${one.engine}/${one.unit}`,
      state: "quota" as const,
      question:
        `${(one.spent_fraction * 100).toFixed(0)}% of the ` +
        `${windowName(one.unit)} window on ${one.engine} is spent`,
      // A QUOTA AGES: without the instant it was read at, a reading from
      // yesterday looks exactly like one from a minute ago.
      context:
        `read ${ago(one.observed_at, now)} · the provider says it resets ` +
        (one.resets_at ?? "at an hour it does not state"),
      since: null,
      runId: null,
      stepId: null,
    }));
}

/** Rank first, then the longest wait: an undated row cannot claim a wait. */
export function inOrder(decisions: Decision[]): Decision[] {
  return [...decisions].sort((left, right) => {
    if (RANK[left.state] !== RANK[right.state]) return RANK[left.state] - RANK[right.state];
    if (left.since === null && right.since === null) return left.id.localeCompare(right.id);
    if (left.since === null) return 1;
    if (right.since === null) return -1;
    if (left.since !== right.since) return left.since - right.since;
    return left.id.localeCompare(right.id);
  });
}

/** What happened unattended: everything that ended and is not a decision. */
export function reportsFrom(history: Execution[], since: number): Report[] {
  const reports: Report[] = [];
  for (const run of endedSince(history, since)) {
    const state = outcomeOfRun(run);
    if (state === null || state === "broke") continue;
    reports.push({
      id: run.run_id,
      state,
      what: `${run.entity} ${STATE_WORD[state]}`,
      context:
        `${String(run.steps_went)} of ${String(run.steps_total)} steps went` +
        (run.error === null ? "" : ` · ${firstLine(run.error)}`),
      at: run.ended_at ?? since,
      costMicros: run.total_cost_micros,
      runId: run.run_id,
    });
  }
  return reports.sort((left, right) => right.at - left.at);
}

/** Every source the screen reads, each carrying its own outcome. */
export interface Sources {
  open: Asked<OpenRun[]>;
  /** Terminals the register calls open that hold nobody. */
  terminals: Asked<Abandoned>;
  handed: Asked<Record<string, HandedStep[]>>;
  history: Asked<Execution[]>;
  quota: Asked<Quota>;
}

/** How many there are, so «all of them are mute» is not a hand-kept number. */
export const HOW_MANY_SOURCES = 5;

/** One source falling silent must not silence the others. */
async function tried<T>(work: Promise<T>): Promise<Asked<T>> {
  try {
    return { state: "answered", value: await work };
  } catch (error) {
    return { state: "mute", why: String(error) };
  }
}

/** The question each stopped run asks, read run by run. */
async function handedFor(open: Asked<OpenRun[]>): Promise<Asked<Record<string, HandedStep[]>>> {
  if (open.state !== "answered") {
    return { state: "mute", why: "the open runs did not answer, so nothing could be asked of them" };
  }
  const waiting = open.value.filter((run) => run.state === "waiting");
  try {
    const answers = await Promise.all(waiting.map((run) => handedSteps(run.run_id)));
    const byRun: Record<string, HandedStep[]> = {};
    waiting.forEach((run, at) => (byRun[run.run_id] = answers[at]));
    return { state: "answered", value: byRun };
  } catch (error) {
    return { state: "mute", why: String(error) };
  }
}

export async function readSources(): Promise<Sources> {
  const [open, history, windows, terminals] = await Promise.all([
    tried(openRuns()),
    tried(executionHistory()),
    tried(quota()),
    tried(abandonedTerminals()),
  ]);
  return { open, history, quota: windows, terminals, handed: await handedFor(open) };
}

/** What could not be read, named. A count with a hole in it is not a count. */
export function blindSpots(sources: Sources): string[] {
  const named: Array<[string, Asked<unknown>]> = [
    ["the open runs", sources.open],
    ["the steps handed to you", sources.handed],
    ["the history of runs", sources.history],
    ["the quota", sources.quota],
    ["the terminals", sources.terminals],
  ];
  const blind: string[] = [];
  for (const [name, asked] of named) {
    if (asked.state === "mute") blind.push(`${name} (${asked.why})`);
  }
  // **A MACHINE THAT WOULD NOT BE QUESTIONED IS A HOLE IN THE COUNT**, not a
  // clean register: the command answered, and its answer is that nobody looked.
  if (sources.terminals.state === "answered" && sources.terminals.value.answer === "could_not_look") {
    const { tool, reason } = sources.terminals.value.refusal;
    blind.push(`the terminals (${tool}: ${reason})`);
  }
  return blind;
}

/** «Three things wait» is a count; with a source mute it is a floor, and says so. */
export function headingOf(waiting: number, partial: boolean): string {
  if (waiting === 0) {
    return partial ? "Nothing I could read is waiting for you" : "Nothing is waiting for you";
  }
  const things = waiting === 1 ? "thing waits" : "things wait";
  return `${partial ? "At least " : ""}${String(waiting)} ${things} for you`;
}

export function subOf(reports: number, historyMute: boolean): string {
  if (historyMute) return "and I could not read what happened while you were away";
  if (reports === 0) return "and nothing happened while you were away";
  const things = reports === 1 ? "thing" : "things";
  return `and ${String(reports)} ${things} happened while you were away`;
}

/** How far back the second section looks, said rather than left to be guessed. */
export function spanOf(since: number, now: number): string {
  const hours = Math.max(1, Math.round((now - since) / 3600));
  return hours === 1 ? "the last hour" : `the last ${String(hours)} hours`;
}
