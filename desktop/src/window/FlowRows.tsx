import { stepCountLabel } from "../flow";
import { groupsHere, type FlowRow, type FlowsReading } from "../flowsbyworkspace";
import { groupWords } from "../flowgroups";
import { t } from "../i18n";

/**
 * **THE PANEL LISTS NAMES, AND A NAME RESOLVES SOMEWHERE.** Grouped by the
 * origin that won, most specific first: two files can carry one name, and a
 * flat list would draw the loser and the winner as the same row.
 */
export function FlowRows({
  reading,
  chosen,
  onChoose,
}: {
  reading: FlowsReading;
  chosen: string | null;
  onChoose: (name: string) => void;
}) {
  const groups = groupsHere(reading);
  if (groups.length === 0) {
    return <p className="window-panel__empty">{t("window.flows.no_flows")}</p>;
  }
  return (
    <>
      {groups.map((group) => (
        <div key={group.key} className="window-rows__group">
          <h2 className="window-rows__head">{groupWords(group.key)}</h2>
          <ul className="window-rows">
            {group.rows.map((row) => (
              <li key={row.name}>
                <button
                  type="button"
                  className="window-row"
                  aria-current={row.name === chosen ? "true" : undefined}
                  onClick={() => { onChoose(row.name); }}
                >
                  <span className="window-row__name">{row.name}</span>
                  <span className="window-row__since">{countWords(row)}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ))}
    </>
  );
}

/** What the row says on its right: how many steps, or that it will not load. */
function countWords(row: FlowRow): string {
  if (row.steps === null) return t("window.flows.broken");
  return stepCountLabel(row.steps);
}
