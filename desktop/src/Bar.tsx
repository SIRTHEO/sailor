// The three facts the bar keeps in sight from every place: what is running,
// what today cost, and who the person is working as.
//
// **IF A RUN IS UNDER WAY YOU MUST KNOW IT FROM ANYWHERE.** Until now the bar
// said so only on the board; leaving it left the run out of sight.

import { useEffect, useState } from "react";
import { liveStatus, openRuns, todaySummary, type DaySummary, type LiveStatus, type OpenRun } from "./engine";
import { rows as profileRows, type Row as ProfileRow } from "./profiles";

const RUNS_EVERY_MS = 4000;
const SPEND_EVERY_MS = 30000;
const WHO_EVERY_MS = 60000;
const BUILD_EVERY_MS = 2000;

/** Polls a question on a cadence; `null` while unanswered or outside the shell. */
function useEvery<T>(native: boolean, ask: () => Promise<T>, every: number): T | null {
  const [answer, setAnswer] = useState<T | null>(null);
  useEffect(() => {
    if (!native) return;
    let alive = true;
    const once = () => {
      ask().then(
        (value) => {
          if (alive) setAnswer(value);
        },
        () => {
          if (alive) setAnswer(null);
        },
      );
    };
    once();
    const timer = setInterval(once, every);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [native, ask, every]);
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

export function LiveChip({ native, now, onOpen }: { native: boolean; now: number; onOpen?: (runId: string) => void }) {
  const runs = useEvery(native, openRuns, RUNS_EVERY_MS);
  const summary = useEvery(native, todaySummary, SPEND_EVERY_MS);
  if (!native) return null;
  const { live, word } = liveWords(runs ?? [], now);
  const first = runs?.[0];
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
      {summary !== null && <span className="chip__spend">{spendWords(summary)}</span>}
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
  return { warn: true, word: `REBUILD FAILED · you see the last good version${since}` };
}

export function BuildChip({ native, now }: { native: boolean; now: number }) {
  const status = useEvery(native, liveStatus, BUILD_EVERY_MS);
  const said = buildWords(status, now);
  if (!native || said === null) return null;
  return (
    <span className="chip chip--build" data-warn={said.warn || undefined} title={status?.message || undefined}>
      {said.word}
    </span>
  );
}

export function WhoChip({ native }: { native: boolean }) {
  const rows = useEvery(native, profileRows, WHO_EVERY_MS);
  if (!native || rows === null) return null;
  const unreachable = rows.some((row) => row.active && row.access === "no");
  return (
    <span className="chip chip--who" data-warn={unreachable || undefined} title="the active profile of each command line">
      {whoWords(rows)}
    </span>
  );
}
