import { since } from "../workspaces";
import type { Project } from "../workspaces";
import { t } from "../i18n";

/**
 * **A ROW SAYS WHAT IT IS, NOT WHAT IT COSTS.** The panel is scanned to find a
 * place; the numbers belong to the thing once it is open.
 */
export function WorkspaceRows({
  projects,
  chosen,
  now,
  onChoose,
}: {
  projects: Project[];
  chosen: string | null;
  /** Passed in, so a row's age is a fact of the render and a test can fix it. */
  now: number;
  onChoose: (root: string) => void;
}) {
  if (projects.length === 0) {
    return <p className="window-panel__empty">{t("window.workspaces.none")}</p>;
  }
  return (
    <ul className="window-rows">
      {projects.map((project) => (
        <li key={project.root}>
          <button
            type="button"
            className="window-row"
            aria-current={project.root === chosen ? "true" : undefined}
            onClick={() => { onChoose(project.root); }}
          >
            <span className="window-row__name">{project.name}</span>
            {project.standing === "gone" ? (
              <span className="window-row__gone">{t("window.workspaces.gone")}</span>
            ) : (
              <span className="window-row__since">{since(project.last_seen, now)}</span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}
