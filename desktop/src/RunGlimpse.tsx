import { useEffect, useState } from "react";
import { runGlimpse, runUsage, type RunGlimpse as Glimpse } from "./engine";
import type { RunUsage } from "./flow";
import { Handed } from "./Handed";
import { t } from "./i18n";
import { Spend } from "./RunConsole";

/** The smallest run view there is: for a run this window's shell never
 * started, this reads the ledger instead of following a live console. */

/** How long a stale «no run called …» stays readable before the view closes
 * itself: long enough to read, short enough not to become a fixture. */
export const GONE_MS = 10_000;

type Ask =
  | { state: "asking" }
  | { state: "ready"; glimpse: Glimpse }
  | { state: "mute"; why: string };

export interface RunGlimpseProps {
  runId: string;
  onClose: () => void;
  /** Called after a gesture the engine accepted, so whoever lists runs reads again. */
  onChanged?: () => void;
  /** Absent, no "Why" is offered: the second level this run's history lives
   * at is reached from here, per the owner's 14/09 "tre voci" — it is not a
   * place of its own any more. */
  onWhy?: () => void;
}

export function RunGlimpse({ runId, onClose, onChanged, onWhy }: RunGlimpseProps) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [usage, setUsage] = useState<RunUsage | null>(null);

  useEffect(() => {
    let alive = true;
    setAsk({ state: "asking" });
    setUsage(null);
    runGlimpse(runId).then(
      (glimpse) => {
        if (alive) setAsk({ state: "ready", glimpse });
      },
      (error: unknown) => {
        if (alive) setAsk({ state: "mute", why: String(error) });
      },
    );
    runUsage(runId).then((found) => {
      if (alive) setUsage(found);
    }, () => {});
    return () => {
      alive = false;
    };
  }, [runId]);

  useEffect(() => {
    if (ask.state !== "mute" || !ask.why.includes("no run called ")) return;
    const timer = setTimeout(onClose, GONE_MS);
    return () => clearTimeout(timer);
  }, [ask, onClose]);

  return (
    <section className="glimpse" aria-label="the run, as the ledger knows it">
      <header className="glimpse__bar">
        <span className="glimpse__title">{t("window.run_glimpse.title")}</span>
        <span className="glimpse__id">{runId}</span>
        <div className="console__spacer" />
        <button type="button" className="console__close" onClick={onClose} title={t("window.run_glimpse.close")}>
          ✕
        </button>
      </header>

      <p className="console__truth">{t("window.run_glimpse.note")}</p>

      {ask.state === "asking" && <p className="glimpse__note">{t("window.run_glimpse.asking")}</p>}
      {ask.state === "mute" && (
        <p className="glimpse__note" data-bad>
          {t("window.run_glimpse.error", { why: ask.why })}
        </p>
      )}
      {ask.state === "ready" && (
        <dl className="glimpse__facts">
          <dt>{t("window.run_glimpse.flow")}</dt>
          <dd>{ask.glimpse.flow.trim() === "" ? t("window.run_glimpse.flow_unknown") : ask.glimpse.flow}</dd>
          {ask.glimpse.worktree !== null && (
            <>
              <dt>{t("window.run_glimpse.worktree")}</dt>
              <dd>{ask.glimpse.worktree}</dd>
            </>
          )}
        </dl>
      )}

      {usage && <Spend usage={usage} />}

      {onWhy && (
        <button type="button" className="glimpse__why" onClick={onWhy}>
          {t("window.run_glimpse.why")}
        </button>
      )}

      <Handed runId={runId} onChanged={onChanged} />
    </section>
  );
}
