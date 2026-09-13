import { useEffect, useState } from "react";
import { t } from "./i18n";
import { stripData, type StripAccount, type StripRun, type StripState } from "./engine";

export interface StripViewProps {
  runs?: StripRun[];
  accounts?: StripAccount[];
  endedToday?: number;
  now: number;
  native?: boolean;
  onOpenRun?: (runId: string) => void;
  onOpenAttention?: () => void;
}

export function formatElapsed(seconds: number): string {
  const s = Math.max(0, seconds);
  if (s < 60) return t("window.strip.seconds", { count: s });
  const m = Math.floor(s / 60);
  if (m < 60) return t("window.strip.minutes", { count: m });
  return t("window.strip.hours_minutes", { hours: Math.floor(m / 60), minutes: m % 60 });
}

export function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(2)}`;
}

export function timeAgo(at: number, now: number): string {
  const gap = Math.max(0, now - at);
  if (gap < 60) return t("window.strip.read_just_now");
  if (gap < 3600) return t("window.strip.read_ago", { time: `${Math.floor(gap / 60)}m` });
  if (gap < 86_400) return t("window.strip.read_ago", { time: `${Math.floor(gap / 3600)}h` });
  return t("window.strip.read_ago", { time: `${Math.floor(gap / 86_400)}d` });
}

export function Strip({
  runs: propRuns,
  accounts: propAccounts,
  endedToday: propEndedToday,
  now,
  native = false,
  onOpenRun,
  onOpenAttention,
}: StripViewProps) {
  const [data, setData] = useState<StripState | null>(null);

  useEffect(() => {
    if (propRuns !== undefined || !native) return;
    let alive = true;
    const fetchStrip = () => {
      stripData(now - (now % 86400)).then(
        (res) => {
          if (alive) setData(res);
        },
        () => undefined,
      );
    };
    fetchStrip();
    const interval = setInterval(fetchStrip, 10_000);
    return () => {
      alive = false;
      clearInterval(interval);
    };
  }, [native, now, propRuns]);

  const runs = propRuns ?? data?.runs ?? [];
  const accounts = propAccounts ?? data?.accounts ?? [];
  const endedToday = propEndedToday ?? data?.ended_today ?? 0;

  const maxSlots = 4;
  const visibleRuns = runs.slice(0, maxSlots);
  const remainingCount = runs.length - maxSlots;

  return (
    <div className="strip" role="region" aria-label={t("window.strip.aria_label")}>
      <div className="strip__accent" aria-hidden="true" />
      <div className="strip__body">
        <div className="strip__runs">
          {runs.length === 0 ? (
            <div className="strip__rest">
              <span className="strip__clear">{t("window.strip.clear")}</span>
              {endedToday > 0 && (
                <span className="strip__ended">
                  {t("window.strip.ended_today", { count: endedToday })}
                </span>
              )}
            </div>
          ) : (
            <div className="strip__slots">
              {visibleRuns.map((run) => {
                const isWaiting = run.state === "waiting";
                const isHolderGone = run.holder_gone || run.state === "holder_gone";
                const isNotYet = run.state === "not_yet";

                const stateLabel = isWaiting
                  ? t("window.strip.waiting_on_you")
                  : isHolderGone
                    ? t("window.strip.holder_gone")
                    : isNotYet
                      ? t("window.strip.not_yet")
                      : t("window.strip.working");

                const showSpend = run.state === "working" && run.cap_micros != null;

                return (
                  <button
                    key={run.run_id}
                    type="button"
                    className="strip__run_cell"
                    data-testid="strip-run-cell"
                    data-state={run.state}
                    onClick={() => onOpenRun?.(run.run_id)}
                  >
                    <span className="strip__run_state" data-state={run.state}>
                      {stateLabel}
                    </span>
                    <span className="strip__run_entity">{run.entity}</span>
                    {run.step && <span className="strip__run_step">{run.step}</span>}
                    <span className="strip__run_elapsed">
                      {formatElapsed(run.elapsed_secs)}
                    </span>
                    {showSpend && (
                      <span className="strip__run_spend">
                        {money(run.spend_micros)}
                      </span>
                    )}
                    {showSpend && run.cap_micros != null && (
                      <span className="strip__run_cap_bar">
                        <span
                          className="strip__run_cap_fill"
                          style={{
                            width: `${Math.min(100, Math.round((run.spend_micros / run.cap_micros) * 100))}%`,
                          }}
                        />
                      </span>
                    )}
                    {run.device && (
                      <span className="strip__run_device">{run.device}</span>
                    )}
                  </button>
                );
              })}
              {remainingCount > 0 && (
                <button
                  type="button"
                  className="strip__more"
                  onClick={onOpenAttention}
                  aria-label={t("window.strip.more", { count: remainingCount })}
                >
                  {t("window.strip.more", { count: remainingCount })}
                </button>
              )}
            </div>
          )}
        </div>

        <div className="strip__rule" aria-hidden="true" />

        <div className="strip__accounts">
          {accounts.map((account) => {
            const hoverTitle = account.unavailable
              ? t("window.strip.account_unavailable", { account: account.name })
              : t("window.strip.account_read", {
                  account: account.name,
                  age: account.read_at != null ? timeAgo(account.read_at, now) : t("window.strip.unavailable"),
                });

            return (
              <div
                key={account.name}
                className="strip__account"
                data-testid={`strip-account-${account.monogram}`}
                data-unavailable={account.unavailable ? "true" : undefined}
                title={hoverTitle}
              >
                <span className="strip__monogram">{account.monogram}</span>
                {account.unavailable ? (
                  <span className="strip__unavailable">
                    {t("window.strip.unavailable")}
                  </span>
                ) : (
                  <div className="strip__track">
                    <div
                      className="strip__track_fill"
                      style={{
                        width: `${Math.min(100, Math.round((account.spent_fraction ?? 0) * 100))}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
