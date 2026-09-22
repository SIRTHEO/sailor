import { useState } from "react";
import { useAsk } from "../ask";
import { flowsHere, resolvedIn, standingContext } from "../flowsbyworkspace";
import { projects } from "../workspaces";
import type { Project } from "../workspaces";
import { t } from "../i18n";
import { Field, FieldEmpty } from "./Field";
import { FlowPage } from "./FlowPage";
import { FlowRows } from "./FlowRows";
import { LISTS, listEntry } from "./lists";
import type { ListName } from "./lists";
import { Panel } from "./Panel";
import { Window } from "./Window";
import { WorkspacePage } from "./WorkspacePage";
import { WorkspaceRows } from "./WorkspaceRows";

const PROJECTS_EVERY_MS = 30000;
const ONCE = null;

/**
 * **A LIST THAT IS NOT HERE YET SAYS WHERE IT IS**, in the field and not in the
 * panel: the panel is scanned for a row, and there is no row — the sentence is
 * an explanation, and an explanation is read. The alternative is a column with
 * icons that answer nothing, which teaches that the window is broken rather
 * than unfinished.
 */
type NotHere = "terminals" | "data" | "keys";

const NOT_YET: Record<NotHere, string> = {
  terminals: "window.not_yet.terminals",
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
export function TheWindow({ native }: { native: boolean }) {
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
