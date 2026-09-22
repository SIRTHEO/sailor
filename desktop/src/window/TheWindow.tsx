import { useState } from "react";
import { useAsk } from "../ask";
import { projects } from "../workspaces";
import type { Project } from "../workspaces";
import { t } from "../i18n";
import { Field } from "./Field";
import { LISTS, listEntry } from "./lists";
import type { ListName } from "./lists";
import { Panel } from "./Panel";
import { Window } from "./Window";
import { WorkspacePage } from "./WorkspacePage";
import { WorkspaceRows } from "./WorkspaceRows";

const PROJECTS_EVERY_MS = 30000;

/**
 * **A LIST THAT IS NOT HERE YET SAYS WHERE IT IS.** The alternative is a column
 * with four icons that answer nothing, which teaches that the window is broken
 * rather than unfinished.
 */
const NOT_YET: Record<ListName, string> = {
  workspaces: "window.not_yet.workspaces",
  flows: "window.not_yet.flows",
  terminals: "window.not_yet.terminals",
  data: "window.not_yet.data",
  keys: "window.not_yet.keys",
};

function notYet(list: ListName): string {
  return t(NOT_YET[list]);
}

export function TheWindow({ native }: { native: boolean }) {
  const [list, setList] = useState<ListName>("workspaces");
  const [chosen, setChosen] = useState<string | null>(null);
  const { asked } = useAsk(native, projects, PROJECTS_EVERY_MS, t("window.outside_the_shell"));

  const found: Project[] = asked.state === "answered" ? asked.value : [];
  const open = found.find((project) => project.root === chosen) ?? null;

  return (
    <Window
      place={list}
      onPlace={(next) => { setList(next); }}
      panel={
        <Panel title={listEntry(list).name}>
          {list !== "workspaces" ? (
            <p className="window-panel__empty">{notYet(list)}</p>
          ) : asked.state === "answered" ? (
            <WorkspaceRows
              projects={found}
              chosen={chosen}
              now={Math.floor(Date.now() / 1000)}
              onChoose={(root) => { setChosen(root); }}
            />
          ) : (
            <p className="window-panel__empty">
              {asked.state === "mute" ? asked.why : t("window.looking")}
            </p>
          )}
        </Panel>
      }
      field={
        open === null ? (
          <Field name={t("window.field.nothing_open")} note={t("window.field.pick_one")}>
            <p className="window-panel__empty">{t("window.field.pick_one")}</p>
          </Field>
        ) : (
          <Field name={open.name} note={open.current ? t("window.field.standing_here") : undefined}>
            <WorkspacePage native={native} project={open} />
          </Field>
        )
      }
    />
  );
}

/** The five names, so a caller can tell what the column offers without React. */
export const OFFERED: ListName[] = LISTS.map((entry) => entry.id);
