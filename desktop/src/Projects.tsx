/**
 * The projects Sailor has been opened in, and what each declares. **A LIST,
 * NOT A SWITCH — YET**: switching moves the root flows resolve against, the
 * open terminals and the credentials in reach. Until that is decided, this
 * shows what is there and where it lives.
 */
import { useCallback, useEffect, useState } from "react";
import { declarationOf, projects, since, type Declaration, type Project } from "./workspaces";

type Ask =
  | { state: "asking" }
  | { state: "asked"; seen: Project[] }
  | { state: "mute"; why: string };

export function Projects({ native, now }: { native: boolean; now: number }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [chosen, setChosen] = useState<string | null>(null);
  const [declared, setDeclared] = useState<Declaration | null>(null);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no home to read" });
      return;
    }
    projects().then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  // The declaration is read for the one project being looked at, never for
  // every row: drawing a list would otherwise touch the disk once per line.
  const look = useCallback((root: string) => {
    setChosen(root);
    setDeclared(null);
    declarationOf(root).then(setDeclared, () => setDeclared(null));
  }, []);

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot read the projects: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Reading the home…</p></div>;
  }
  if (ask.seen.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">
          No project has been opened yet. Run <code>sailor workspace init</code> inside one,
          and it appears here.
        </p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Projects</h2>
        <span className="now__count">{ask.seen.length}</span>
      </header>
      <table className="now__table">
        <thead>
          <tr><th>project</th><th>where</th><th>opened</th><th>since</th></tr>
        </thead>
        <tbody>
          {ask.seen.map((project) => (
            <tr
              key={project.root}
              data-here={project.current || undefined}
              data-gone={project.standing === "gone" || undefined}
              onClick={() => look(project.root)}
            >
              <td className="now__entity">
                {project.name}
                {project.current && <span className="rail__note"> — you are here</span>}
              </td>
              {/* THE PATH IS NOT DECORATION. A project whose marker has gone is
                  repaired by knowing where it was, and nothing else says it. */}
              <td className="now__path">{project.root}</td>
              <td>{since(project.last_seen, now)}</td>
              <td>{since(project.first_seen, now)}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* A PROJECT THAT LOST ITS MARKER IS SHOWN, NOT DROPPED: a list that
          quietly shrinks cannot be told from one that never had the entry. */}
      {ask.seen.some((project) => project.standing === "gone") && (
        <p className="now__mute">
          The rows marked gone have no <code>sailor.json</code> where they were left —
          moved, renamed, or deleted. The path is kept so they can be found again.
        </p>
      )}

      {chosen !== null && declared !== null && (
        <section className="panel__block">
          <div className="panel__title">What {declared.name || "it"} declares</div>
          <dl className="now__kv">
            <dt>rules</dt>
            <dd>{declared.rules.length > 0 ? declared.rules.join(" · ") : "none declared"}</dd>
            <dt>checks</dt>
            <dd>
              {Object.keys(declared.checks).length > 0
                ? Object.entries(declared.checks).map(([name]) => name).join(" · ")
                : "none declared"}
            </dd>
            <dt>equipment</dt>
            <dd>{declared.equipment ?? "none declared"}</dd>
          </dl>
        </section>
      )}
    </div>
  );
}
