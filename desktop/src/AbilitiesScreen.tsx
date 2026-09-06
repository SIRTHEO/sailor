/**
 * Every action the engine can run, **with the policy it runs under**. The list
 * was names grouped by a family this window computes itself, so nothing on it
 * came from the engine but the words. The surface and the powers of
 * `docs/the-four-surfaces.md` are not drawn: nobody declares them, fault 67.
 */
import { useEffect, useState } from "react";
import { invoker } from "./engine";
import { KNOWN_ACTIONS, kindOf, type StepKind } from "./flow";

/** One registered action as the shell sends it: the contract is `flows.rs`. */
export interface Registered {
  name: string;
  redo: string;
  closes_a_run: boolean;
}

type Ask = { state: "asking" } | { state: "asked"; seen: Registered[] } | { state: "mute"; why: string };

/** What redoing an action costs, in the words a person reads. */
export const REDO: Record<string, string> = {
  repeatable: "run it again",
  compensable: "undo, then run it again",
  hand_to_human: "hand it to a person",
};

/** What the canvas draws for an action, or that it has nothing of its own. */
export function drawnAs(action: string): { kind: StepKind; own: boolean } {
  return { kind: kindOf(action), own: KNOWN_ACTIONS.includes(action) };
}

export function AbilitiesScreen({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no engine to ask" });
      return;
    }
    const invoke = invoker();
    if (!invoke) return;
    invoke<Registered[]>("engine_actions").then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot ask the engine: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Asking the engine what it registers…</p></div>;
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">What Sailor can do</h2>
        <span className="now__count">{ask.seen.length}</span>
      </header>
      <p className="now__mute">
        Asked of the running engine, not read from a list kept here. These are the words a
        flow file may use for a step’s <code>action</code>, and how the engine treats each —
        its own conservative defaults included.
      </p>

      <table className="now__table">
        <thead>
          <tr><th>action</th><th>if it has to be redone</th><th>may close a run</th><th>drawn as</th></tr>
        </thead>
        <tbody>
          {ask.seen.map((one) => {
            const drawn = drawnAs(one.name);
            return (
              <tr key={one.name}>
                <td className="now__entity"><code>{one.name}</code></td>
                <td>{REDO[one.redo] ?? one.redo}</td>
                <td>{one.closes_a_run ? "yes" : "no"}</td>
                {/* THE DRAWING IS NOT THE AUTHORITY: `kindOf` guesses from the
                    name, and beside fields the engine answered an unmarked
                    guess reads as one more thing the engine said. */}
                <td>
                  {drawn.own ? (
                    drawn.kind
                  ) : (
                    <span className="now__why" data-bad>no node of its own · drawn as {drawn.kind}</span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <p className="now__why">
        Nothing here says which surface an action belongs to — <code>sense</code>,{" "}
        <code>act</code>, <code>remember</code>, <code>gate</code> — nor whether it reaches the
        network, the disk, processes or a secret: no registered action declares any of it,
        and that contract is deferred, in <code>docs/the-four-surfaces.md</code> and fault 67.
        Money is the one power the engine really enforces, and it is enforced per step,
        against the parameters that step declares, so it is not a fact about an action.
      </p>
    </div>
  );
}
