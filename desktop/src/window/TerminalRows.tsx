import { t } from "../i18n";
import { livenessOf, livenessWord, type TerminalSummary } from "../terminal";
import { terminalGroups } from "./terminalgroups";

/**
 * **THE PANEL LISTS THE TERMINALS, THE FIELD HOLDS ONE.** A row names what runs
 * in it and says whether it is still alive, in a word: the tint beside it is a
 * machine's, and never the only thing saying so.
 */
export function TerminalRows({
  terminals,
  held,
  closed,
  watching,
  speaking,
  onChoose,
}: {
  terminals: TerminalSummary[];
  /** The one in the field, so its row carries the mark. */
  held: string | null;
  closed: ReadonlyMap<string, string>;
  watching: boolean;
  speaking: ReadonlySet<string>;
  onChoose: (id: string) => void;
}) {
  if (terminals.length === 0) {
    return <p className="window-panel__empty">{t("window.terminals.none")}</p>;
  }
  return (
    <>
      {terminalGroups(terminals).map((group) => (
        <div key={group.root} className="window-rows__group">
          <h2 className="window-rows__head" title={group.root}>{group.head}</h2>
          <ul className="window-rows">
            {group.terminals.map((terminal) => {
              const liveness = livenessOf(terminal, closed, watching);
              return (
                <li key={terminal.id}>
                  <button
                    type="button"
                    className="window-row"
                    aria-current={terminal.id === held ? "true" : undefined}
                    onClick={() => { onChoose(terminal.id); }}
                  >
                    <span className="window-row__name">{terminal.program === "" ? terminal.device : terminal.program}</span>
                    <span className="window-row__since" data-liveness={liveness.state}>
                      {livenessWord(liveness, speaking.has(terminal.id))}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </>
  );
}
