import { useEffect, useState } from "react";
import type { Execution, RunEvent, RunSnapshot } from "./engine";
import { ledgerQuery } from "./ledger";
import { t } from "./i18n";
import type { Ask } from "./QuotaScreen";

/** Discard late responses when the person changes session or run. */
export function useReading<T>(active: boolean, read: () => Promise<T>, every: number): Ask<T> {
  const [answer, setAnswer] = useState<Ask<T>>({ state: "asking" });
  useEffect(() => {
    if (!active) return;
    let gone = false;
    let timer: ReturnType<typeof setTimeout>;
    setAnswer({ state: "asking" });
    const ask = async () => {
      try {
        const seen = await read();
        if (!gone) setAnswer({ state: "asked", seen });
      } catch (error) {
        if (!gone) setAnswer({ state: "mute", why: String(error) });
      }
      if (!gone) timer = setTimeout(ask, every);
    };
    void ask();
    return () => { gone = true; clearTimeout(timer); };
  }, [active, read, every]);
  return answer;
}

/** Read persisted attempts, including work started outside this window. */
export async function recordedRun(run: Pick<Execution, "run_id" | "entity" | "started_at" | "status">): Promise<{ run: RunSnapshot; truncated: boolean }> {
  const id = run.run_id.replace(/'/g, "''");
  const answer = await ledgerQuery(`SELECT step_id, attempt, started_at, ended_at, input, output, outcome, said, failure_class, refusal, ran FROM steps WHERE run_id = '${id}' ORDER BY started_at DESC, step_id, attempt DESC`);
  const events: RunEvent[] = [];
  const json = (value: unknown): unknown => typeof value === "string" ? JSON.parse(value) : null;
  for (const row of [...answer.rows].reverse()) {
    const [step, attempt, start, end, input, output, outcome, said, failure, refusal, ran] = row;
    const stepId = t("window.session.attempt", { step: String(step), attempt: Number(attempt) });
    const base = { run_id: run.run_id, step_id: stepId };
    events.push({ ...base, seq: events.length, kind: "step_started", at: Number(start), payload: { input: json(input) } });
    if (outcome !== null) events.push({ ...base, seq: events.length, kind: "step_closed", at: Number(end ?? start), payload: {
      outcome, said, failure_class: failure, input: json(input), output: json(output), refusal: json(refusal), ran: json(ran),
    } });
  }
  events.sort((a, b) => a.at - b.at || a.seq - b.seq);
  return { run: { run_id: run.run_id, flow: run.entity, started_at: run.started_at, status: run.status, events }, truncated: answer.truncated };
}
