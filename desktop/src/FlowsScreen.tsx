/**
 * **WHAT RUNS HERE, AND IS ANYTHING REPLACING IT.** One table over the shell's
 * reading: the checkout the window stands in by default, every workspace on
 * request, and a name two checkouts resolve differently is never one row.
 */
import { useEffect, useState } from "react";
import { chainWords, replacesWords } from "./flowchain";
import {
  flowsByWorkspace,
  globalRows,
  groupsHere,
  holdsAnyFlow,
  runsWhereTheWindowStands,
  standingContext,
  type FlowContext,
  type FlowRow,
  type FlowsAsk,
  type FlowsReading,
} from "./flowsbyworkspace";
import { t } from "./i18n";
import { treeName } from "./workspacetrees";

export interface FlowsViewProps {
  ask: FlowsAsk;
  /** Opens the flow on the board: only offered for the file that runs here. */
  onOpen?: (name: string) => void;
  onRun?: (name: string) => void;
}

type Mode = "here" | "all";

const GROUP_WORDS: Record<string, string> = {
  workspace: "window.flows.group.workspace",
  declared: "window.flows.group.declared",
  yours: "window.flows.group.yours",
  "built in": "window.flows.group.built_in",
};

/** Where a context stands, in the words a person scans for. */
export function contextWords(context: FlowContext): string {
  if (context.root === null) return t("window.flows.outside");
  return t("window.flows.checkout", {
    workspace: context.workspace ?? treeName(context.root),
    tree: treeName(context.root),
    branch: context.branch ?? t("window.flows.no_branch"),
  });
}

function Steps({ row }: { row: FlowRow }) {
  if (row.steps === null) {
    return (
      <span className="flows__broken" title={row.broken}>
        {t("window.flows.broken")}
      </span>
    );
  }
  return <>{row.steps}</>;
}

function Replaces({ row }: { row: FlowRow }) {
  const words = replacesWords(row);
  if (words === null) return null;
  return (
    <span className="rail__replaces" title={chainWords(row)}>
      {words}
    </span>
  );
}

interface Chosen {
  row: FlowRow;
  context: FlowContext;
}

function Detail({
  reading,
  chosen,
  onOpen,
  onRun,
}: {
  reading: FlowsReading;
  chosen: Chosen | null;
  onOpen?: (name: string) => void;
  onRun?: (name: string) => void;
}) {
  if (chosen === null) {
    return (
      <aside className="flows__detail">
        <p className="flows__mute">{t("window.flows.detail.none")}</p>
      </aside>
    );
  }
  const { row, context } = chosen;
  const here = runsWhereTheWindowStands(reading, row);
  return (
    <aside className="flows__detail" aria-label={t("window.flows.detail.label")}>
      <h2 className="flows__name">{row.name}</h2>
      <p className="flows__where">{contextWords(context)}</p>
      <h3 className="flows__label">{t("window.flows.detail.file")}</h3>
      <p className="flows__path">{row.winner.path}</p>
      <h3 className="flows__label">{t("window.flows.detail.chain")}</h3>
      <pre className="flows__chain">{chainWords(row)}</pre>
      {row.broken !== undefined && <p className="flows__broken">{row.broken}</p>}
      {here ? (
        <div className="flows__actions">
          {onOpen && (
            <button type="button" className="flows__action" onClick={() => onOpen(row.name)}>
              {t("window.flows.detail.open")}
            </button>
          )}
          {onRun && row.broken === undefined && (
            <button type="button" className="flows__action" onClick={() => onRun(row.name)}>
              {t("window.flows.detail.run")}
            </button>
          )}
        </div>
      ) : (
        <p className="flows__mute">{t("window.flows.detail.elsewhere")}</p>
      )}
    </aside>
  );
}

function Unreadable({ contexts }: { contexts: FlowContext[] }) {
  const refused = contexts.filter((one) => one.unreadable !== undefined);
  if (refused.length === 0) return null;
  return (
    <ul className="flows__troubles">
      {refused.map((one) => (
        <li key={one.root ?? ""}>
          {t("window.flows.context_unreadable", { root: one.root ?? "", why: one.unreadable ?? "" })}
        </li>
      ))}
    </ul>
  );
}

export function FlowsView({ ask, onOpen, onRun }: FlowsViewProps) {
  const [mode, setMode] = useState<Mode>("here");
  const [chosen, setChosen] = useState<Chosen | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());

  const head = (
    <>
      <h1 className="flows__asks">{t("window.flows.asks")}</h1>
      <div className="flows__modes">
        {(["here", "all"] as Mode[]).map((one) => (
          <button
            type="button"
            key={one}
            className="flows__mode"
            aria-pressed={mode === one}
            onClick={() => {
              setMode(one);
              setChosen(null);
            }}
          >
            {t(`window.flows.mode.${one}`)}
          </button>
        ))}
      </div>
    </>
  );

  if (ask.state === "reading") {
    return (
      <div className="flows">
        {head}
        <p className="flows__mute">{t("window.flows.reading")}</p>
      </div>
    );
  }
  if (ask.state === "unreadable") {
    return (
      <div className="flows">
        {head}
        <p className="flows__broken" role="alert">
          {t("window.flows.unreadable", { why: ask.why })}
        </p>
      </div>
    );
  }

  const { reading } = ask;
  const standing = standingContext(reading);
  const isChosen = (row: FlowRow, context: FlowContext) =>
    chosen !== null && chosen.row.name === row.name && chosen.context === context;

  const rowOf = (row: FlowRow, context: FlowContext, extra?: React.ReactNode) => (
    <tr
      key={`${context.root ?? ""}\n${row.name}`}
      className="flows__row"
      data-chosen={isChosen(row, context) || undefined}
      onClick={() => setChosen({ row, context })}
    >
      <td>{row.name}</td>
      <td className="flows__steps">
        <Steps row={row} />
      </td>
      <td className="flows__origin">{row.winner.origin}</td>
      <td>
        <Replaces row={row} />
      </td>
      {extra}
    </tr>
  );

  const columns = (
    <tr>
      <th>{t("window.flows.column.name")}</th>
      <th>{t("window.flows.column.steps")}</th>
      <th>{t("window.flows.column.origin")}</th>
      <th>{t("window.flows.column.replaces")}</th>
      {mode === "all" && <th>{t("window.flows.column.contexts")}</th>}
    </tr>
  );

  const here = (
    <>
      <p className="flows__where">{contextWords(standing)}</p>
      {standing.unreadable !== undefined ? (
        <Unreadable contexts={[standing]} />
      ) : (
        <table className="flows__table">
          <thead>{columns}</thead>
          {groupsHere(reading).map((group) => (
            <tbody key={group.key}>
              <tr className="flows__group">
                <th colSpan={4}>{GROUP_WORDS[group.key] ? t(GROUP_WORDS[group.key]) : group.key}</th>
              </tr>
              {group.rows.map((row) => rowOf(row, standing))}
            </tbody>
          ))}
        </table>
      )}
    </>
  );

  const all = (
    <>
      <table className="flows__table">
        <thead>{columns}</thead>
        <tbody>
          {globalRows(reading).map((one) => {
            if (!one.differs) {
              const first = one.winners[0];
              return rowOf(
                first.row,
                first.context,
                <td className="flows__origin">{t("window.flows.in_places", { count: one.winners.length })}</td>,
              );
            }
            const expanded = open.has(one.name);
            return [
              <tr key={one.name} className="flows__row" data-differs>
                <td>{one.name}</td>
                <td colSpan={3}>
                  <button
                    type="button"
                    className="flows__expand"
                    aria-expanded={expanded}
                    onClick={() =>
                      setOpen((was) => {
                        const next = new Set(was);
                        if (next.has(one.name)) next.delete(one.name);
                        else next.add(one.name);
                        return next;
                      })
                    }
                  >
                    {t("window.flows.differs", { count: one.winners.length })}
                  </button>
                </td>
                <td />
              </tr>,
              ...(expanded
                ? one.winners.map((winner) =>
                    rowOf(winner.row, winner.context, <td className="flows__sub">{contextWords(winner.context)}</td>),
                  )
                : []),
            ];
          })}
        </tbody>
      </table>
      <Unreadable contexts={reading.contexts} />
    </>
  );

  return (
    <div className="flows">
      {head}
      {reading.contexts.length === 0 && <p className="flows__mute">{t("window.flows.no_workspace")}</p>}
      {!holdsAnyFlow(reading) && <p className="flows__mute">{t("window.flows.no_flows")}</p>}
      <div className="flows__body">
        <div className="flows__list">{mode === "here" ? here : all}</div>
        <Detail reading={reading} chosen={chosen} onOpen={onOpen} onRun={onRun} />
      </div>
    </div>
  );
}

export interface FlowsScreenProps {
  native: boolean;
  onOpen?: (name: string) => void;
  onRun?: (name: string) => void;
}

/** Read on arrival: coming back to the place is what refreshes it. */
export function FlowsScreen({ native, onOpen, onRun }: FlowsScreenProps) {
  const [ask, setAsk] = useState<FlowsAsk>(() =>
    native ? { state: "reading" } : { state: "unreadable", why: t("window.flows.no_shell") },
  );
  useEffect(() => {
    if (!native) return;
    let watching = true;
    flowsByWorkspace()
      .then((reading) => {
        if (watching) setAsk({ state: "read", reading });
      })
      .catch((error: unknown) => {
        if (watching) setAsk({ state: "unreadable", why: String(error) });
      });
    return () => {
      watching = false;
    };
  }, [native]);
  return <FlowsView ask={ask} onOpen={onOpen} onRun={onRun} />;
}
