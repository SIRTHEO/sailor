import { useAsk } from "../ask";
import { declarationOf } from "../workspaces";
import type { Project } from "../workspaces";
import { t } from "../i18n";

const ONCE = null;

/**
 * What the workspace itself declares, and nothing this window made up. A key
 * the declaration does not carry is named as absent: a blank cell reads as
 * «none», and «none» is a different fact from «never said».
 */
export function WorkspacePage({ native, project }: { native: boolean; project: Project }) {
  const { asked } = useAsk(
    native,
    () => declarationOf(project.root),
    ONCE,
    t("window.outside_the_shell"),
  );

  return (
    <dl className="window-facts">
      <dt>{t("window.workspace.folder")}</dt>
      <dd className="window-facts__path">{project.root}</dd>
      {asked.state === "asking" && <dd>{t("window.looking")}</dd>}
      {asked.state === "mute" && <dd className="window-facts__absent">{asked.why}</dd>}
      {asked.state === "answered" && (
        <>
          <dt>{t("window.workspace.rules")}</dt>
          <dd>
            {asked.value.rules.length === 0
              ? <span className="window-facts__absent">{t("window.workspace.no_rules")}</span>
              : asked.value.rules.join(", ")}
          </dd>
          <dt>{t("window.workspace.checks")}</dt>
          <dd>
            {Object.keys(asked.value.checks).length === 0
              ? <span className="window-facts__absent">{t("window.workspace.no_checks")}</span>
              : Object.keys(asked.value.checks).join(", ")}
          </dd>
          <dt>{t("window.workspace.equipment")}</dt>
          <dd>
            {asked.value.equipment === null
              ? <span className="window-facts__absent">{t("window.workspace.no_equipment")}</span>
              : asked.value.equipment}
          </dd>
        </>
      )}
    </dl>
  );
}
