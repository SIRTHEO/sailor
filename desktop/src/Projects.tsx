/**
 * The projects Sailor has been opened in, what each declares, and the gesture
 * that moves the window into one. Moving changes the root the flows, the runs
 * and the census resolve against; the open terminals keep the tree they were
 * opened in, because a terminal belongs to its workspace and not to the window.
 */
import { useCallback, useEffect, useState } from "react";
import { declarationOf, projects, since, workHere, type Declaration, type Project } from "./workspaces";

type Ask =
  | { state: "asking" }
  | { state: "asked"; seen: Project[] }
  | { state: "mute"; why: string };

interface ProjectsProps {
  native: boolean;
  now: number;
  /** Called once the window has moved into another project, so the flows are read again. */
  onMoved?: () => void;
}

export function Projects({ native, now, onMoved }: ProjectsProps) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [chosen, setChosen] = useState<string | null>(null);
  const [declared, setDeclared] = useState<Declaration | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);

  const read = useCallback(() => {
    projects().then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no home to read" });
      return;
    }
    read();
  }, [native, read]);

  // The move is asked of the engine, and the list is read again from it: the
  // row is not marked «here» by the window, or a refused move would look done.
  const move = useCallback(
    (root: string) => {
      setTrouble(null);
      workHere(root).then(
        () => {
          read();
          onMoved?.();
        },
        (error) => setTrouble(String(error)),
      );
    },
    [read, onMoved],
  );

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
      {trouble && <p className="now__mute">The move was refused: {trouble}</p>}
      <table className="now__table">
        <thead>
          <tr><th>project</th><th>where</th><th>opened</th><th>since</th><th /></tr>
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
              <td>
                {!project.current && project.standing === "declared" && (
                  <button
                    type="button"
                    className="now__act"
                    onClick={(event) => {
                      event.stopPropagation();
                      move(project.root);
                    }}
                  >
                    work here
                  </button>
                )}
              </td>
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
