import { waitedFor } from "./waiting";
import { t } from "./i18n";
import { rankAttentionRows, type AttentionRow } from "./attention";

export interface AttentionQueueProps {
  rows: AttentionRow[];
  now: number;
  onRun?: (runId: string) => void;
  onTty?: (tty: string) => void;
}

export function AttentionQueue({ rows, now, onRun, onTty }: AttentionQueueProps) {
  if (rows.length === 0) return null;
  const sorted = rankAttentionRows(rows);

  return (
    <section className="waiting__group">
      <h3 className="waiting__section">{t("window.attention.title")}</h3>
      <div>
        {sorted.map((row, index) => {
          const key = row.step_id
            ? `${row.run_id ?? "run"}/${row.step_id}`
            : row.run_id ?? (row.tty ? `terminal/${row.tty}` : `attention-${String(index)}`);
          const elapsed = row.since != null ? waitedFor(row.since, now) : "";
          const reason =
            row.reason && row.reason.trim() !== "" ? row.reason : "reason unknown";
          const statusWord =
            row.kind === "handed"
              ? row.status_word
              : row.status_word === "waiting on you"
                ? "stopped"
                : row.status_word;

          const link = row.link;

          return (
            <article key={key} className="waiting__row" data-kind={row.kind}>
              <div className="waiting__text">
                <b className="waiting__what">{reason}</b>
                <span className="waiting__context">
                  <span className="waiting__word">{statusWord}</span>
                </span>
              </div>
              {elapsed !== "" && <span className="waiting__when">{elapsed}</span>}
              <div className="waiting__acts">
                {link && link.kind === "run" && onRun !== undefined && (
                  <button
                    type="button"
                    className="waiting__act waiting__act--primary"
                    onClick={() => onRun(link.run_id)}
                  >
                    {t("window.attention.open_run")}
                  </button>
                )}
                {link && link.kind === "tty" && onTty !== undefined && (
                  <button
                    type="button"
                    className="waiting__act waiting__act--primary"
                    onClick={() => onTty(link.tty)}
                  >
                    {t("window.attention.open_terminal", { tty: link.tty })}
                  </button>
                )}
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
