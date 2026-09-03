/**
 * Which workspace the window is in, and the gesture that moves it. A workspace
 * has **several trees** — one project checked out once per branch — so the name
 * groups and the trees sit under it.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { projects, workHere, type Project } from "./workspaces";

type Ask =
  | { state: "asking" }
  | { state: "asked"; seen: Project[] }
  | { state: "mute"; why: string };

/** A project and the trees it is checked out into, most recent first. */
export interface Grouped {
  name: string;
  trees: Project[];
}

/** Groups the trees under the name their marker declares. */
export function grouped(seen: Project[]): Grouped[] {
  const byName = new Map<string, Project[]>();
  for (const one of seen) {
    const trees = byName.get(one.name);
    if (trees) trees.push(one);
    else byName.set(one.name, [one]);
  }
  return Array.from(byName, ([name, trees]) => ({
    name,
    trees: trees.slice().sort((left, right) => right.last_seen - left.last_seen),
  })).sort((left, right) => right.trees[0].last_seen - left.trees[0].last_seen);
}

/** The last segment of a path: what a person calls that tree. */
export function treeName(root: string): string {
  const parts = root.split("/").filter((part) => part !== "");
  return parts[parts.length - 1] ?? root;
}

/**
 * The two lines of the head, per state. A column 146px wide holds no sentence,
 * and not knowing is not the same answer as being outside.
 */
export function whereWeAre(ask: Ask, here: Project | null): { name: string; tree: string } {
  if (ask.state === "asking") return { name: "…", tree: "" };
  if (ask.state === "mute") return { name: "unknown", tree: "cannot read" };
  if (here === null) return { name: "no workspace", tree: "outside" };
  return { name: here.name, tree: treeName(here.root) };
}

export function WorkspaceSwitcher({
  native,
  onMoved,
}: {
  native: boolean;
  /** Called once the window has moved, so the flows are read again. */
  onMoved: () => void;
}) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [open, setOpen] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const box = useRef<HTMLDivElement | null>(null);

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

  // A menu that stays open behind the next click is a menu in the way.
  useEffect(() => {
    if (!open) return;
    function away(event: MouseEvent) {
      if (!box.current?.contains(event.target as Node)) setOpen(false);
    }
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [open]);

  const seen = ask.state === "asked" ? ask.seen : [];
  const here = seen.find((one) => one.current) ?? null;
  const said = whereWeAre(ask, here);

  function move(root: string) {
    setTrouble(null);
    workHere(root).then(
      () => {
        setOpen(false);
        read();
        onMoved();
      },
      (error: unknown) => setTrouble(String(error)),
    );
  }

  return (
    <div className="wsp" ref={box}>
      <div className="wsp__heading">the workspace</div>
      <button
        type="button"
        className="wsp__here"
        data-open={open || undefined}
        onClick={() => setOpen((was) => !was)}
        disabled={ask.state !== "asked"}
        title={ask.state === "mute" ? ask.why : here?.root}
      >
        <span className="wsp__name">{said.name}</span>
        <span className="wsp__tree">{said.tree}</span>
      </button>

      {open && (
        <div className="wsp__list" role="menu">
          {seen.length === 0 && (
            <div className="wsp__none">
              No project on record. Run <code>sailor workspace init</code> in one.
            </div>
          )}
          {grouped(seen).map((project) => (
            <div className="wsp__group" key={project.name}>
              <div className="wsp__project">{project.name}</div>
              {project.trees.map((tree) => (
                <button
                  type="button"
                  key={tree.root}
                  className="wsp__tree-row"
                  data-here={tree.current || undefined}
                  data-gone={tree.standing === "gone" || undefined}
                  onClick={() => move(tree.root)}
                  title={tree.root}
                >
                  <span className="wsp__tree-name">{treeName(tree.root)}</span>
                  {tree.standing === "gone" && <span className="wsp__gone">gone</span>}
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
      {trouble !== null && <div className="wsp__trouble">{trouble}</div>}
    </div>
  );
}
