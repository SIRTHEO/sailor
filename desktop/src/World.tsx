/**
 * The column is the world: workspaces, the trees each is checked out into, and
 * what lives in every tree. **A list of places is not a navigation**: five
 * nouns say what the program has, not where the work is — and a thing can sit
 * outside every workspace, which is a place of its own.
 */
import { useCallback, useEffect, useState } from "react";
import { PLACES, type Section } from "./Rail";
import { grouped, treeName } from "./Workspace";
import { projects, workHere, type Project } from "./workspaces";
import type { TerminalSummary } from "./terminal";

/** A tree with what Sailor has open in it. */
export interface Inhabited {
  tree: Project;
  terminals: TerminalSummary[];
}

/**
 * Which tree a terminal belongs to: the deepest declared root that contains it.
 * Deepest, because trees nest — a checkout can live inside another project's
 * folder — and the nearest root is the one whose marker governs the work.
 */
export function treeOf(root: string, trees: Project[]): Project | null {
  let best: Project | null = null;
  for (const tree of trees) {
    if (root !== tree.root && !root.startsWith(`${tree.root}/`)) continue;
    if (best === null || tree.root.length > best.root.length) best = tree;
  }
  return best;
}

/** The terminals no workspace claims: outside is a place, not an absence. */
export function outside(terminals: TerminalSummary[], trees: Project[]): TerminalSummary[] {
  return terminals.filter((one) => treeOf(one.workspaceRoot, trees) === null);
}

/** How long ago, in the words a person uses when scanning a column. */
export function ago(seconds: number, now: number): string {
  const gap = Math.max(0, now - seconds);
  if (gap < 90) return "now";
  const minutes = Math.round(gap / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h`;
  return `${Math.round(hours / 24)}d`;
}

export function World({
  native,
  here,
  onGo,
  counts,
  terminals,
  onMoved,
}: {
  native: boolean;
  here: Section;
  onGo: (section: Section) => void;
  counts: Partial<Record<Section, number>>;
  terminals: TerminalSummary[];
  /** Called once the window has moved into another tree. */
  onMoved: () => void;
}) {
  const [seen, setSeen] = useState<Project[]>([]);
  const [why, setWhy] = useState<string | null>(null);
  const [folded, setFolded] = useState<Set<string>>(new Set());

  const read = useCallback(() => {
    projects().then(
      (found) => {
        setSeen(found);
        setWhy(null);
      },
      (error) => setWhy(String(error)),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      setWhy("outside the desktop shell there is no home to read");
      return;
    }
    read();
  }, [native, read]);

  function move(root: string) {
    workHere(root).then(
      () => {
        read();
        onMoved();
        onGo("board");
      },
      (error: unknown) => setWhy(String(error)),
    );
  }

  const homeless = outside(terminals, seen);
  // No tree open means the board is not under one: it still has to be reachable.
  const noTreeOpen = !seen.some((one) => one.current);

  return (
    <nav className="world" aria-label="the world">
      {/* What holds wherever you are. Terminals stays here though every live
          one is a leaf below: with none open there would be no way to open
          the first. */}
      <div className="world__above">
        {PLACES.filter((place) => place.id !== "board").map((place) => (
          <button
            type="button"
            key={place.id}
            className="world__global"
            data-here={here === place.id || undefined}
            onClick={() => onGo(place.id)}
            title={place.asks}
          >
            <span className="world__glyph" aria-hidden="true">
              {place.glyph}
            </span>
            <span className="world__label">{place.name}</span>
          </button>
        ))}
      </div>

      <div className="world__head">workspaces</div>

      {why !== null && <div className="world__mute">{why}</div>}

      {grouped(seen).map((project) => {
        const shut = folded.has(project.name);
        return (
          <div className="wsx" key={project.name}>
            <button
              type="button"
              className="wsx__name"
              onClick={() =>
                setFolded((was) => {
                  const next = new Set(was);
                  if (shut) next.delete(project.name);
                  else next.add(project.name);
                  return next;
                })
              }
            >
              <span className="wsx__fold" aria-hidden="true">
                {shut ? "›" : "▾"}
              </span>
              {project.name}
            </button>
            {!shut &&
              project.trees.map((tree) => (
                <div className="wsx__tree" key={tree.root}>
                  <button
                    type="button"
                    className="wsx__row"
                    data-here={tree.current || undefined}
                    data-gone={tree.standing === "gone" || undefined}
                    onClick={() => move(tree.root)}
                    title={tree.root}
                  >
                    <span className="wsx__dot" data-live={tree.current || undefined} />
                    <span className="wsx__tree-name">{treeName(tree.root)}</span>
                    {tree.standing === "gone" && <span className="wsx__gone">gone</span>}
                  </button>
                  {tree.current && (
                    <button
                      type="button"
                      className="wsx__leaf"
                      data-here={here === "board" || undefined}
                      onClick={() => onGo("board")}
                    >
                      <span className="world__glyph" aria-hidden="true">
                        ◈
                      </span>
                      <span className="world__label">Board</span>
                      {counts.board !== undefined && (
                        <span className="wsx__count">{counts.board}</span>
                      )}
                    </button>
                  )}
                  {terminals
                    .filter((one) => treeOf(one.workspaceRoot, seen)?.root === tree.root)
                    .map((one) => (
                      <button
                        type="button"
                        key={one.id}
                        className="wsx__leaf"
                        onClick={() => onGo("terminals")}
                        title={one.program || one.device}
                      >
                        <span className="wsx__dot" data-live={one.alive || undefined} />
                        <span className="wsx__term">{one.device}</span>
                        <span className="wsx__count">{one.program}</span>
                      </button>
                    ))}
                </div>
              ))}
          </div>
        );
      })}

      {/* Outside is a place: a terminal no project claims is where a good
          deal of the work happens. The board lives here when no tree is
          open — unreachable is worse than in the wrong group. */}
      <div className="world__head">outside every workspace</div>
      {noTreeOpen && (
        <button
          type="button"
          className="wsx__leaf"
          data-here={here === "board" || undefined}
          onClick={() => onGo("board")}
        >
          <span className="world__glyph" aria-hidden="true">
            ◈
          </span>
          <span className="world__label">Board</span>
          {counts.board !== undefined && <span className="wsx__count">{counts.board}</span>}
        </button>
      )}
      {homeless.length === 0 && !noTreeOpen && (
        <div className="world__mute">nothing open out here</div>
      )}
      {homeless.map((one) => (
        <button
          type="button"
          key={one.id}
          className="wsx__leaf"
          onClick={() => onGo("terminals")}
          title={one.workspaceRoot}
        >
          <span className="wsx__dot" data-live={one.alive || undefined} />
          <span className="wsx__term">{one.device}</span>
          <span className="wsx__count">{treeName(one.workspaceRoot)}</span>
        </button>
      ))}
    </nav>
  );
}
