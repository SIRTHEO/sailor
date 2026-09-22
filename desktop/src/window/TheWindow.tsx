import { useState } from "react";
import { useAsk } from "../ask";
import { flowsHere, resolvedIn, standingContext } from "../flowsbyworkspace";
import { projects } from "../workspaces";
import type { Project } from "../workspaces";
import { t } from "../i18n";
import { closeTerminal, livenessOf, pressKeys, progressOf, resizeTerminal, submitLine } from "../terminal";
import { TerminalPane, useStir } from "../TerminalPane";
import { Field, FieldEmpty, FieldHeld } from "./Field";
import { FlowPage } from "./FlowPage";
import { FlowRows } from "./FlowRows";
import { LISTS, listEntry } from "./lists";
import type { ListName } from "./lists";
import { Panel } from "./Panel";
import { heldTerminal } from "./terminalgroups";
import { TerminalRows } from "./TerminalRows";
import { useTerminals } from "./useTerminals";
import { Window } from "./Window";
import { WorkspacePage } from "./WorkspacePage";
import { WorkspaceRows } from "./WorkspaceRows";

const PROJECTS_EVERY_MS = 30000;
const ONCE = null;

/**
 * **A LIST THAT IS NOT HERE YET SAYS WHERE IT IS**, in the field and not in the
 * panel: the panel is scanned for a row, and there is no row. Icons that answer
 * nothing teach that the window is broken rather than unfinished.
 */
type NotHere = "data" | "keys";

const NOT_YET: Record<NotHere, string> = {
  data: "window.not_yet.data",
  keys: "window.not_yet.keys",
};

/** Where a list stands: which one the column points at, and how to move. */
interface At {
  place: ListName;
  onPlace: (place: ListName) => void;
  /** What is chosen in THIS list, kept by name so leaving and coming back
   *  reopens what was open rather than the empty field. */
  chosen: string | null;
  onChoose: (what: string) => void;
}

/**
 * **ONE LIST IS MOUNTED AT A TIME**, and it asks for its own answer. Every list
 * rendered together would read the disk five times to draw one column, and four
 * of the five readings would be thrown away.
 */
export function TheWindow({
  native,
  ceiling = null,
}: {
  native: boolean;
  /** The ceiling the relay hands on at, from the flow that declares it. */
  ceiling?: number | null;
}) {
  const [place, setPlace] = useState<ListName>("workspaces");
  const [chosen, setChosen] = useState<Partial<Record<ListName, string>>>({});
  const at: At = {
    place,
    onPlace: (next) => { setPlace(next); },
    chosen: chosen[place] ?? null,
    onChoose: (what) => { setChosen((all) => ({ ...all, [place]: what })); },
  };

  if (place === "workspaces") return <Workspaces native={native} at={at} />;
  if (place === "flows") return <Flows native={native} at={at} />;
  if (place === "terminals") return <Terminals native={native} ceiling={ceiling} at={at} />;
  return <NotHereYet list={place} at={at} />;
}

function Workspaces({ native, at }: { native: boolean; at: At }) {
  const { asked } = useAsk(native, projects, PROJECTS_EVERY_MS, t("window.outside_the_shell"));
  const found: Project[] = asked.state === "answered" ? asked.value : [];
  const open = found.find((project) => project.root === at.chosen) ?? null;

  return (
    <Window
      place={at.place}
      onPlace={at.onPlace}
      panel={
        <Panel title={listEntry(at.place).name}>
          {asked.state === "answered" ? (
            <WorkspaceRows
              projects={found}
              chosen={at.chosen}
              now={Math.floor(Date.now() / 1000)}
              onChoose={at.onChoose}
            />
          ) : (
            <Waiting asked={asked} />
          )}
        </Panel>
      }
      field={
        open === null ? (
          <FieldEmpty say={t("window.field.pick_one")} />
        ) : (
          <Field name={open.name} note={open.current ? t("window.field.standing_here") : undefined}>
            <WorkspacePage native={native} project={open} />
          </Field>
        )
      }
    />
  );
}

function Flows({ native, at }: { native: boolean; at: At }) {
  // READ ON ARRIVAL, NOT ON A BEAT: resolving every name in every source walks
  // the disk, and a flow file changes when somebody saves one, not every 30 s.
  const { asked } = useAsk(native, flowsHere, ONCE, t("window.outside_the_shell"));
  const open =
    asked.state === "answered"
      ? resolvedIn(asked.value, standingContext(asked.value)).find((row) => row.name === at.chosen) ?? null
      : null;

  return (
    <Window
      place={at.place}
      onPlace={at.onPlace}
      panel={
        <Panel title={listEntry(at.place).name}>
          {asked.state === "answered" ? (
            <FlowRows reading={asked.value} chosen={at.chosen} onChoose={at.onChoose} />
          ) : (
            <Waiting asked={asked} />
          )}
        </Panel>
      }
      field={
        open === null ? (
          <FieldEmpty say={t("window.flows.detail.none")} />
        ) : (
          <Field name={open.name} note={open.winner.origin}>
            <FlowPage row={open} />
          </Field>
        )
      }
    />
  );
}

/**
 * **ONE TERMINAL IN THE FIELD, AND NO TABS.** The panel is the list, so a
 * strip of tabs over the pane would be a second navigation; and the field
 * shows what is open, whole, rather than a card of facts about it.
 */
function Terminals({ native, ceiling, at }: { native: boolean; ceiling: number | null; at: At }) {
  const { asked, again, bus, closed, channel, speaking, now, lines } = useTerminals(native);
  const [trouble, setTrouble] = useState<string | null>(null);
  const all = asked.state === "answered" ? asked.value : [];
  const held = heldTerminal(all, at.chosen);
  const stirred = useStir(held?.workspaceRoot ?? null);
  const refused = (error: unknown) => { setTrouble(String(error)); };

  let field;
  if (held === null) {
    field = <FieldEmpty say={t(asked.state === "answered" ? "window.terminals.open_one" : "window.looking")} />;
  } else {
    const liveness = livenessOf(held, closed, channel.on);
    field = (
      <FieldHeld name={`${held.program === "" ? held.device : held.program} · ${held.workspaceName}`}>
        {trouble === null ? null : <p className="window-field__trouble">{trouble}</p>}
        {channel.why === null ? null : <p className="window-field__trouble">{channel.why}</p>}
        <TerminalPane
          key={held.id}
          summary={held}
          known={lines}
          ceiling={ceiling}
          liveness={liveness}
          progress={progressOf(liveness, bus.spokenAt(held.id), now)}
          stirred={stirred}
          speaking={speaking.has(held.id)}
          bus={bus}
          visible
          focused
          onFocus={() => {}}
          onSubmit={(line) => submitLine(held.id, line)}
          onClose={() => { void closeTerminal(held.id).then(() => { again(); }).catch(refused); }}
          onPress={(bytes) => { void pressKeys(held.id, bytes).catch(refused); }}
          onResize={(cols, rows) => { void resizeTerminal(held.id, cols, rows).catch(refused); }}
        />
      </FieldHeld>
    );
  }

  return (
    <Window
      place={at.place}
      onPlace={at.onPlace}
      panel={
        <Panel title={listEntry(at.place).name}>
          {asked.state === "answered" ? (
            <TerminalRows
              terminals={all}
              held={held?.id ?? null}
              closed={closed}
              watching={channel.on}
              speaking={speaking}
              onChoose={(id) => { setTrouble(null); at.onChoose(id); }}
            />
          ) : (
            <Waiting asked={asked} />
          )}
        </Panel>
      }
      field={field}
    />
  );
}

function NotHereYet({ list, at }: { list: NotHere; at: At }) {
  return (
    <Window
      place={at.place}
      onPlace={at.onPlace}
      panel={<Panel title={listEntry(at.place).name}>{null}</Panel>}
      field={<FieldEmpty say={t(NOT_YET[list])} />}
    />
  );
}

/** Still asking, or refused with the reason: never an empty list for either. */
function Waiting({ asked }: { asked: { state: "asking" } | { state: "mute"; why: string } }) {
  return (
    <p className="window-panel__empty">
      {asked.state === "mute" ? asked.why : t("window.looking")}
    </p>
  );
}

/** The five names, so a caller can tell what the column offers without React. */
export const OFFERED: ListName[] = LISTS.map((entry) => entry.id);
