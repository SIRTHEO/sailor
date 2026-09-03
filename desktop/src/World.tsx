/**
 * The column is the world: workspaces, the trees each is checked out into, and
 * what lives in every tree — board, flows, terminals. **A list of places is
 * not a navigation**: five nouns say what the program has, not where the work
 * is. A thing can also sit outside every workspace, a place of its own.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { PLACES, type Section } from "./Rail";
import { grouped, treeName } from "./Workspace";
import { projects, workHere, type Project } from "./workspaces";
import type { TerminalSummary } from "./terminal";

/**
 * The flows of one source, as the column draws them. The order is the
 * engine's: least specific first, the last name wins — sorting here would stop
 * the column matching what runs.
 */
export interface FlowGroup {
  origin: string | null;
  flows: { name: string; note: string; color?: string; dirty: boolean }[];
  broken: { name: string; reason: string }[];
}

/** Where a source's flows belong in the column. */
export const OF_THIS_TREE = "this project";

/**
 * The flows a tree owns: its own, and the ones saved nowhere yet. They hang
 * under the tree because that is what they answer to — the other sources are
 * the same wherever you stand, so they get a place of their own.
 */
export function ofTheTree(groups: FlowGroup[]): FlowGroup[] {
  return groups.filter((group) => group.origin === OF_THIS_TREE || group.origin === null);
}

/** Everything else: one list per source, in the order the engine gave them. */
export function everywhere(groups: FlowGroup[]): FlowGroup[] {
  return groups.filter((group) => group.origin !== OF_THIS_TREE && group.origin !== null);
}

/** What the column writes over flows that belong to no disk yet. */
const NOT_SAVED = "not saved yet";

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

/**
 * The tree to reopen in, or `null` when there is nothing to do. Nothing when
 * one is already open — reopening then would drag you out of the tree you
 * chose — and nothing when no project is on record.
 */
export function reopenIn(seen: Project[]): Project | null {
  if (seen.length === 0 || seen.some((one) => one.current)) return null;
  return seen.reduce((newest, one) => (one.last_seen > newest.last_seen ? one : newest));
}

export function World({
  native,
  here,
  onGo,
  counts,
  terminals,
  onMoved,
  flowGroups,
  focusName,
  onFlow,
  onNewFlow,
}: {
  native: boolean;
  here: Section;
  onGo: (section: Section) => void;
  counts: Partial<Record<Section, number>>;
  terminals: TerminalSummary[];
  /** Called once the window has moved into another tree. */
  onMoved: () => void;
  flowGroups: FlowGroup[];
  focusName: string | null;
  onFlow: (name: string | null) => void;
  onNewFlow: () => void;
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
      setWhy("no home to read out here");
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

  // **THE WINDOW REOPENS WHERE YOU LEFT OFF.** Which tree is current is read
  // from the process's directory, and a window launched from the Finder starts
  // in `/`: every project on the list and none of them open. Once, and only
  // when nothing is current — otherwise it would drag you back out of a tree
  // you chose.
  const settled = useRef(false);
  useEffect(() => {
    if (settled.current) return;
    const last = reopenIn(seen);
    if (seen.length === 0) return;
    settled.current = true;
    if (last !== null) move(last.root);
    // `move` is stable enough for this: it is called once, on the first list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seen]);

  // The list of one source, wherever in the column that source belongs. The
  // classes are the flow column's own: it did not change, it moved.
  function flowsOf(group: FlowGroup) {
    return (
      <div className="rail__group" key={group.origin ?? NOT_SAVED}>
        <div className="rail__origin">{group.origin ?? NOT_SAVED}</div>
        {group.flows.map((one) => (
          <button
            type="button"
            key={one.name}
            className="rail__item"
            data-open={one.name === focusName || undefined}
            onClick={() => onFlow(one.name)}
          >
            <span className="rail__dot" style={{ background: one.color }} />
            <span className="rail__label">
              {one.name}
              {one.dirty && <span className="rail__dirty-dot" title="not saved" />}
            </span>
            <span className="rail__note">{one.note}</span>
          </button>
        ))}
        {/* A broken flow does not vanish from the list: it is shown, marked,
            with the reason. It stays off the canvas because it has no graph to
            draw — and it stays under its source, which is where whoever goes
            to repair it has to look. */}
        {group.broken.map((one) => (
          <div className="rail__item" key={one.name} data-broken>
            <span className="rail__label">{one.name}</span>
            <span className="rail__note">{one.reason}</span>
          </div>
        ))}
      </div>
    );
  }

  const mine = ofTheTree(flowGroups);
  const shared = everywhere(flowGroups);
  const anyFlow = flowGroups.some((group) => group.flows.length > 0);
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
                    <>
                      <button
                        type="button"
                        className="wsx__leaf"
                        data-here={here === "board" || undefined}
                        onClick={() => {
                          onGo("board");
                          onFlow(null);
                        }}
                      >
                        <span className="world__glyph" aria-hidden="true">
                          ◈
                        </span>
                        <span className="world__label">Board</span>
                        {counts.board !== undefined && (
                          <span className="wsx__count">{counts.board}</span>
                        )}
                      </button>
                      {mine.map(flowsOf)}
                      {anyFlow && (
                        <button type="button" className="rail__new" onClick={onNewFlow}>
                          + New flow
                        </button>
                      )}
                    </>
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

      {/* THE FLOWS THAT ARE THE SAME WHEREVER YOU STAND. Yours, and the ones
          that ship inside the binary: neither belongs under a tree, and buried
          in a column of «this project» they read as one more checkout's. */}
      {shared.length > 0 && <div className="world__head">flows everywhere</div>}
      {shared.map(flowsOf)}

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
      {noTreeOpen && mine.map(flowsOf)}
      {noTreeOpen && anyFlow && (
        <button type="button" className="rail__new" onClick={onNewFlow}>
          + New flow
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
