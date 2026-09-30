import type { FlowRow } from "../flowsbyworkspace";
import { t } from "../i18n";

/**
 * **A NAME IS NOT A FILE.** What the panel listed is the name that resolves
 * here; this says which file that name reaches and which files it beat, so a
 * flow running differently from the one you edited is readable rather than a
 * surprise at the next run.
 */
export function FlowPage({ row }: { row: FlowRow }) {
  return (
    <dl className="window-facts">
      <dt>{t("window.flows.detail.file")}</dt>
      <dd className="window-facts__path">{row.winner.path}</dd>

      <dt>{t("window.flows.column.origin")}</dt>
      <dd>{row.winner.origin}</dd>

      <dt>{t("window.flows.column.steps")}</dt>
      <dd>
        {row.steps === null ? (
          <span className="window-facts__absent">{row.broken ?? t("window.flows.broken")}</span>
        ) : (
          row.steps
        )}
      </dd>

      <dt>{t("window.flows.detail.chain")}</dt>
      <dd>
        {row.replaced.length === 0 ? (
          <span className="window-facts__absent">{t("window.flows.detail.replaces_nothing")}</span>
        ) : (
          <ul className="window-facts__chain">
            {row.replaced.map((one) => (
              <li key={`${one.origin}:${one.path}`}>
                {t("window.flow.chain_replaced", { origin: one.origin, path: one.path })}
              </li>
            ))}
          </ul>
        )}
      </dd>
    </dl>
  );
}
