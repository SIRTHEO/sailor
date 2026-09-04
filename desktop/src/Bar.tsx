// The three facts the bar keeps in sight from every place: what is running,
// what today cost, and who the person is working as.
//
// **IF A RUN IS UNDER WAY YOU MUST KNOW IT FROM ANYWHERE.** Until now the bar
// said so only on the board; leaving it left the run out of sight.

import { useEffect, useRef, useState } from "react";
import {
  beatReport,
  listenToSailorEvents,
  liveStatus,
  openRuns,
  takeNewBuild,
  todaySummary,
  type BeatReport,
  type DaySummary,
  type LiveStatus,
  type OpenRun,
} from "./engine";
import { rows as profileRows, type Row as ProfileRow } from "./profiles";

/**
 * What a question has answered so far. **A REFUSAL IS AN ANSWER**, and it is
 * kept: a chip that read a failed ask as «nothing» would say «nothing
 * running» about an engine that is not answering — fault 30 in the bar.
 */
export type Answer<T> = { state: "answered"; value: T } | { state: "mute"; why: string } | null;

/**
 * The least time between two asks of the same reader. **A RUN PUTS HUNDREDS OF
 * FACTS A SECOND HERE**, one per piece of an engine's output, and one of the
 * reads behind a chip starts a command per profile.
 */
export const AT_MOST_EVERY_MS = 1_000;

/** What each reader of the bar listens for. */
const OF_A_RUN = ["run", "beat"] as const;
const OF_A_BUILD = ["build"] as const;
const OF_A_BEAT = ["beat"] as const;
/**
 * **NOTHING A RUN SAYS CHANGES WHO YOU RUN AS**, and this read starts a command
 * per profile: hearing anything else pays that price for news it cannot carry.
 */
const OF_A_PROFILE = ["profile"] as const;

/** Whether a fact is one this reader listens for. No list is every kind. */
export function hears(kind: string, kinds?: readonly string[]): boolean {
  return kinds === undefined || kinds.includes(kind);
}

/**
 * Asks once, then again at the facts this reader listens for: what the bar
 * shows moves when the shell says something moved, at most once a second, and
 * the last fact of a burst is asked for at the end of that second rather than
 * dropped. Where the channel cannot be listened to, the first answer stays.
 */
function useOnEvents<T>(native: boolean, ask: () => Promise<T>, kinds?: readonly string[]): Answer<T> {
  const [answer, setAnswer] = useState<Answer<T>>(null);
  const listensFor = useRef(kinds);
  listensFor.current = kinds;
  useEffect(() => {
    if (!native) return;
    let alive = true;
    let askedAt = 0;
    let later: ReturnType<typeof setTimeout> | null = null;
    const once = () => {
      askedAt = Date.now();
      ask().then(
        (value) => {
          if (alive) setAnswer({ state: "answered", value });
        },
        (error: unknown) => {
          if (alive) setAnswer({ state: "mute", why: String(error) });
        },
      );
    };
    const maybe = (kind: string) => {
      if (!alive || !hears(kind, listensFor.current)) return;
      const since = Date.now() - askedAt;
      if (since >= AT_MOST_EVERY_MS) {
        once();
        return;
      }
      if (later !== null) return;
      later = setTimeout(() => {
        later = null;
        if (alive) once();
      }, AT_MOST_EVERY_MS - since);
    };
    once();
    let stop: (() => void) | null = null;
    listenToSailorEvents((event) => maybe(event.kind)).then((subscribed) => {
      if (!alive) {
        if ("stop" in subscribed) subscribed.stop();
        return;
      }
      if ("stop" in subscribed) stop = subscribed.stop;
    });
    return () => {
      alive = false;
      if (later !== null) clearTimeout(later);
      if (stop) stop();
    };
  }, [native, ask]);
  return answer;
}

export function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(2)}`;
}

export function elapsed(since: number, now: number): string {
  const seconds = Math.max(0, now - since);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** The words for what is running: the oldest open run, and how many more. */
export function liveWords(runs: OpenRun[], now: number): { live: boolean; word: string } {
  const working = runs.filter((run) => run.state === "working");
  const waiting = runs.filter((run) => run.state === "waiting");
  if (working.length === 0 && waiting.length === 0) return { live: false, word: "nothing running" };
  const first = working[0] ?? waiting[0];
  const step = first.open_now[0]?.step_id;
  const parts = [first.entity];
  if (first.steps_total !== null) parts.push(`${first.steps_done} of ${first.steps_total}`);
  if (step !== undefined) parts.push(`at ${step}`);
  parts.push(elapsed(first.since, now));
  const more = working.length + waiting.length - 1;
  if (more > 0) parts.push(`+${more} more`);
  if (working.length === 0) parts.push("waiting for you");
  return { live: working.length > 0, word: parts.join(" · ") };
}

export function spendWords(summary: DaySummary | null): string {
  if (summary === null) return "";
  if (!summary.ledger_present) return "no ledger yet";
  const floor = summary.unpriced > 0 ? " (a floor)" : "";
  return `${money(summary.cost_micros)} today${floor}`;
}

interface LiveChipProps {
  native: boolean;
  now: number;
  onOpen?: (runId: string) => void;
  /** The spend is a line of the bar that opens in Memory: this is the opening. */
  onSpend?: () => void;
}

export function LiveChip({ native, now, onOpen, onSpend }: LiveChipProps) {
  const runs = useOnEvents(native, openRuns, OF_A_RUN);
  const summary = useOnEvents(native, todaySummary, OF_A_RUN);
  if (!native) return null;
  if (runs?.state === "mute") {
    return (
      <span className="chip chip--live" data-warn="true">
        cannot ask what runs: {runs.why}
      </span>
    );
  }
  const list = runs?.state === "answered" ? runs.value : [];
  const { live, word } = runs === null ? { live: false, word: "asking the engine…" } : liveWords(list, now);
  const first = list[0];
  return (
    <span className="chip chip--live">
      <span className="topbar__live" data-idle={live ? undefined : true} />
      {first !== undefined && onOpen ? (
        <button type="button" className="chip__button" onClick={() => onOpen(first.run_id)}>
          {word}
        </button>
      ) : (
        <span className="chip__word">{word}</span>
      )}
      {summary?.state === "answered" &&
        (onSpend ? (
          <button type="button" className="chip__button chip__spend" onClick={onSpend} title="open the spend in Memory">
            {spendWords(summary.value)}
          </button>
        ) : (
          <span className="chip__spend">{spendWords(summary.value)}</span>
        ))}
      {summary?.state === "mute" && (
        <span className="chip__spend" data-warn="true">
          cost unknown: {summary.why}
        </span>
      )}
    </span>
  );
}

/** Who the terminals and the engines run as: the active profile of each line. */
export function whoWords(rows: ProfileRow[]): string {
  const active = rows.filter((row) => row.active).map((row) => `${row.cli_id} ${row.name}`);
  return active.length === 0 ? "no profile active" : active.join(" · ");
}

/**
 * The build under the window, in words. Silent while it runs the latest
 * build and outside live mode: a chip that always says «fine» teaches nobody
 * to read it. A failed rebuild says what is being looked at and since when.
 */
export function buildWords(status: LiveStatus | null, now: number): { warn: boolean; word: string } | null {
  if (status === null || status.state === "running") return null;
  if (status.state === "building") return { warn: false, word: "rebuilding the window…" };
  const since = status.running_since === null ? "" : ` running since ${elapsed(status.running_since, now)} ago`;
  // **NOT A WARNING.** Nothing is wrong: a build is done and this window is
  // the one before it, which is exactly what was asked for.
  if (status.state === "ready") return { warn: false, word: `a new build is waiting${since}` };
  return { warn: true, word: `REBUILD FAILED · you see the last good version${since}` };
}

/**
 * How often the deadline beats: `beat::EVERY` on the other side, and here only
 * the yardstick for «late», so a drifting copy warns early, never wrong.
 */
export const BEAT_EVERY_SECS = 60;

/**
 * The beat, in words. **SILENT WHILE THE SCHEDULE IS KEPT**: most decisions are
 * «not due», and a chip reciting them teaches nobody to read it. Two things are
 * worth a person — a due flow that would not start, and a beat that stopped,
 * after which every schedule is off and nothing else here would say so.
 */
export function beatWords(report: BeatReport | null, now: number): { warn: boolean; word: string } | null {
  if (report === null) return null;
  // The lateness comes first: it is the fact that makes the rest old news.
  if (now - report.at > 2 * BEAT_EVERY_SECS) {
    return { warn: true, word: `the beat stopped · last one ${elapsed(report.at, now)} ago` };
  }
  const broke = report.decisions.filter((one) => one.verdict === "broke");
  if (broke.length === 0) return null;
  const first = `${broke[0].flow}: ${broke[0].why ?? "no reason given"}`;
  return {
    warn: true,
    word: broke.length === 1 ? `did not start · ${first}` : `${broke.length} did not start · ${first}`,
  };
}

export function BeatChip({ native, now }: { native: boolean; now: number }) {
  const report = useOnEvents(native, beatReport, OF_A_BEAT);
  if (!native || report === null) return null;
  if (report.state === "mute") {
    return (
      <span className="chip chip--beat" data-warn="true">
        the beat cannot be asked: {report.why}
      </span>
    );
  }
  const said = beatWords(report.value, now);
  if (said === null) return null;
  return (
    <span className="chip chip--beat" data-warn="true">
      {said.word}
    </span>
  );
}

export function BuildChip({ native, now }: { native: boolean; now: number }) {
  const status = useOnEvents(native, liveStatus, OF_A_BUILD);
  if (!native || status === null) return null;
  if (status.state === "mute") {
    return (
      <span className="chip chip--build" data-warn="true">
        build status unknown: {status.why}
      </span>
    );
  }
  const said = buildWords(status.value, now);
  if (said === null) return null;
  // THE ONLY THING IN THIS BAR THAT ENDS THE WINDOW, so it is a button and
  // looks like one: nothing here takes the screen away on its own.
  if (status.value?.state === "ready") {
    return (
      <button
        type="button"
        className="chip chip--build chip--asks"
        onClick={() => void takeNewBuild()}
        title="the window is replaced by the build that is waiting"
      >
        {said.word} · take it
      </button>
    );
  }
  return (
    <span className="chip chip--build" data-warn={said.warn || undefined} title={status.value?.message || undefined}>
      {said.word}
    </span>
  );
}

export function WhoChip({ native }: { native: boolean }) {
  const rows = useOnEvents(native, profileRows, OF_A_PROFILE);
  if (!native || rows === null) return null;
  if (rows.state === "mute") {
    return (
      <span className="chip chip--who" data-warn="true">
        who runs is unknown: {rows.why}
      </span>
    );
  }
  const unreachable = rows.value.some((row) => row.active && row.access === "no");
  return (
    <span className="chip chip--who" data-warn={unreachable || undefined} title="the active profile of each command line">
      {whoWords(rows.value)}
    </span>
  );
}
