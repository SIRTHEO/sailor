/**
 * Where work happens: one tree per branch, and a terminal opened on it. Until
 * now this was git typed by hand, so nothing Sailor records knew which tree a
 * run had happened in.
 */
import { useCallback, useEffect, useState } from "react";
import { openTerminal } from "./terminal";
import { createTree, listTrees, removeTree, type Tree } from "./worktree";

const BORN_COLS = 100;
const BORN_ROWS = 30;

type Ask = { state: "asking" } | { state: "asked"; trees: Tree[] } | { state: "mute"; why: string };

export function Worktrees({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [branch, setBranch] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);

  const again = useCallback(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no repository to read" });
      return;
    }
    listTrees().then(
      (trees) => setAsk({ state: "asked", trees }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  useEffect(again, [again]);

  const cut = useCallback(async () => {
    const wanted = branch.trim();
    if (wanted === "") return;
    setBusy("creating");
    setTrouble(null);
    try {
      await createTree(wanted);
      setBranch("");
      again();
    } catch (error) {
      setTrouble(String(error));
    } finally {
      setBusy(null);
    }
  }, [branch, again]);

  // Git refuses while a tree holds uncommitted work, and that refusal is what
  // reaches the person: it names what would be lost.
  const takeDown = useCallback(
    async (name: string) => {
      setBusy(name);
      setTrouble(null);
      try {
        await removeTree(name);
        again();
      } catch (error) {
        setTrouble(String(error));
      } finally {
        setBusy(null);
      }
    },
    [again],
  );

  const work = useCallback(async (tree: Tree) => {
    setTrouble(null);
    try {
      await openTerminal({ workspaceRoot: tree.path, cols: BORN_COLS, rows: BORN_ROWS });
    } catch (error) {
      setTrouble(String(error));
    }
  }, []);

  if (ask.state === "asking") return <div className="trees__note">reading the repository…</div>;
  if (ask.state === "mute") return <div className="trees__note">{ask.why}</div>;

  return (
    <section className="trees">
      <header className="trees__cut">
        <input
          className="trees__branch"
          value={branch}
          placeholder="a branch to cut a tree for"
          onChange={(event) => setBranch(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && cut()}
        />
        <button type="button" className="trees__do" disabled={branch.trim() === "" || busy !== null} onClick={cut}>
          {busy === "creating" ? "cutting…" : "cut a tree"}
        </button>
      </header>

      {trouble && <div className="trees__trouble">{trouble}</div>}

      <div className="trees__list">
        {ask.trees.map((tree) => (
          <article className="tree" key={tree.path} data-current={tree.current || undefined}>
            <div className="tree__who">
              <span className="tree__name">{tree.name}</span>
              <span className="tree__branch">{tree.branch ?? "detached"}</span>
              {tree.current && <span className="tree__here">this window</span>}
              {tree.locked && <span className="tree__flag">locked</span>}
              {tree.prunable && <span className="tree__flag">its directory is gone</span>}
            </div>
            <div className="tree__path">{tree.path}</div>
            <div className="tree__do">
              <button type="button" className="tree__button" onClick={() => work(tree)}>
                open a terminal here
              </button>
              {/* Never on its own tree: the window would pull the floor out
                  from under itself, and git would allow it. */}
              {!tree.current && (
                <button
                  type="button"
                  className="tree__button"
                  disabled={busy !== null}
                  onClick={() => takeDown(tree.name)}
                >
                  {busy === tree.name ? "taking down…" : "take down"}
                </button>
              )}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
