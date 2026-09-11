import { useCallback, useState } from "react";
import { executionHistory, runUsage, stopRun, type Execution } from "./engine";
import { Calls } from "./History";
import { t } from "./i18n";
import type { CommandLine } from "./profiles";
import { engineOf, quota, windowName } from "./quota";
import { QuotaScreen } from "./QuotaScreen";
import { costReading, panesFromEvents, RunConsole, type ConsoleMode } from "./RunConsole";
import { recordedRun, useReading } from "./recordedWork";
import { StepRefusal } from "./StepRefusal";
import type { TerminalSummary } from "./terminal";

type Detail = "outcome" | "refusals" | "cost" | "quota";

export function SessionContext({ native, terminal, lines }: { native: boolean; terminal: TerminalSummary; lines: CommandLine[] }) {
  const history = useReading(native, executionHistory, 3000);
  const windows = useReading(native, quota, 60000);
  const [picked, setPicked] = useState<string | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null);
  const runs = history.state === "asked" ? history.seen.filter((run) => run.worktree === terminal.workspaceRoot).sort((a, b) => b.started_at - a.started_at) : [];
  const selected = runs.find((run) => run.run_id === picked) ?? runs[0];
  const engine = lines.find((line) => line.executable === terminal.program || line.id === terminal.program)?.id ?? terminal.program;
  // «engine · account» since the reading became per account.
  const matching = windows.state === "asked"
    ? windows.seen.windows.filter((one) => engineOf(one.engine) === engine || engineOf(one.engine) === terminal.program)
    : [];
  const quotaText = windows.state === "mute" ? t("window.session.unreadable", { why: windows.why })
    : windows.state === "asking" ? t("window.session.asking")
    : matching.length === 0 ? t("window.session.quota_unknown", { engine })
    : matching.map((one) => t("window.session.remaining", { window: windowName(one.unit), percent: ((1 - one.spent_fraction) * 100).toFixed(1), at: new Date(one.observed_at * 1000).toLocaleTimeString() })).join(" · ");

  return <aside className="session-context" aria-label={t("window.session.attention")}>
    <label className="session-context__choice">
      {t("window.session.run_in", { folder: terminal.workspaceName })}
      <select value={selected?.run_id ?? ""} onChange={(event) => setPicked(event.target.value)}>
        {runs.length === 0 && <option value="">{history.state === "asked" ? t("window.session.no_run") : t("window.session.asking")}</option>}
        {runs.map((run) => <option key={run.run_id} value={run.run_id}>{run.entity} · {run.run_id} · {run.status}</option>)}
      </select>
    </label>
    <p>{t("window.session.scope")}</p>
    {history.state === "mute" && <p role="status">{t("window.session.unreadable", { why: history.why })}</p>}
    {history.state === "asking" && <p role="status">{t("window.session.asking")}</p>}
    {selected ? <RunAttention key={selected.run_id} native={native} selected={selected} detail={detail} onDetail={setDetail} /> : <>
      {(["outcome", "refusals", "cost"] as const).map((kind) => <button key={kind} className="session-context__signal" type="button" aria-expanded={detail === kind} onClick={() => setDetail(detail === kind ? null : kind)}>
        <strong>{t(`window.session.${kind}`)}</strong><span>{t("window.session.no_selected_run")}</span>
      </button>)}
      {detail !== null && detail !== "quota" && <p className="session-context__detail">{t("window.session.no_selected_run")}</p>}
    </>}
    <button className="session-context__signal" type="button" aria-expanded={detail === "quota"} onClick={() => setDetail(detail === "quota" ? null : "quota")}>
      <strong>{t("window.session.quota")}</strong><span>{quotaText}</span>
    </button>
    {detail === "quota" && <QuotaScreen native={native} now={Date.now() / 1000} readings={windows.state === "asked" ? { state: "asked", seen: { windows: matching, unreachable: windows.seen.unreachable } } : windows} />}
  </aside>;
}

function RunAttention({ native, selected, detail, onDetail }: { native: boolean; selected: Execution; detail: Detail | null; onDetail: (detail: Detail | null) => void }) {
  const { run_id, entity, started_at, status } = selected;
  const read = useCallback(() => recordedRun({ run_id, entity, started_at, status }), [run_id, entity, started_at, status]);
  const readCost = useCallback(() => runUsage(run_id), [run_id]);
  const record = useReading(native, read, 3000);
  const cost = useReading(native, readCost, 3000);
  const [mode, setMode] = useState<ConsoleMode>("split");
  const run = record.state === "asked" ? record.seen.run : null;
  const panes = run ? panesFromEvents(run.events) : [];
  const closed = panes.filter((pane) => pane.endedAt !== null).sort((a, b) => b.endedAt! - a.endedAt!);
  const refusals = panes.filter((pane) => pane.refusal !== null);
  const unknown = record.state === "mute" ? t("window.session.unreadable", { why: record.why }) : t("window.session.asking");
  const summaries: Record<Exclude<Detail, "quota">, string> = {
    outcome: run ? closed[0] ? `${closed[0].stepId} · ${closed[0].outcome}` : t("window.session.no_outcome") : unknown,
    refusals: run ? t(record.state === "asked" && record.seen.truncated ? "window.session.refusals_partial" : "window.session.refusals_count", { count: refusals.length }) : unknown,
    cost: cost.state === "mute" ? t("window.session.unreadable", { why: cost.why }) : cost.state === "asked" && cost.seen ? costReading(cost.seen) : t("window.session.cost_unknown"),
  };
  return <>
    {(["outcome", "refusals", "cost"] as const).map((kind) => <button key={kind} className="session-context__signal" type="button" aria-expanded={detail === kind} onClick={() => onDetail(detail === kind ? null : kind)}>
      <strong>{t(`window.session.${kind}`)}</strong><span>{summaries[kind]}</span>
    </button>)}
    {detail !== null && detail !== "quota" && <div className="session-context__detail">
      <button type="button" onClick={() => onDetail(null)}>{t("window.session.close")}</button>
      {record.state === "asked" && record.seen.truncated && <p role="status">{t("window.session.truncated")}</p>}
      {detail === "outcome" && (run ? <RunConsole run={run} runs={[run]} mode={mode} now={Date.now() / 1000} listenFailure={t("window.session.recorded")} usage={null} onMode={setMode} onPick={() => {}} onClose={() => onDetail(null)} onStop={() => stopRun(run.run_id)} /> : <p>{unknown}</p>)}
      {detail === "refusals" && (run ? refusals.length ? refusals.map((pane) => <div key={pane.stepId}><p>{pane.stepId}</p><StepRefusal refusal={pane.refusal!} /></div>) : <p>{summaries.refusals}</p> : <p>{unknown}</p>)}
      {detail === "cost" && <><p>{summaries.cost}</p><Calls calls={selected.calls} /></>}
    </div>}
  </>;
}
