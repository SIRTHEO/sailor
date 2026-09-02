/**
 * Every action the engine can run, asked of the engine. **NOT THE WINDOW'S
 * VOCABULARY**: `KNOWN_ACTIONS` serves the panel's suggestions and a test keeps
 * it honest, but that is a check, not a screen — «what can this thing do» had
 * no answer anybody could read.
 */
import { useEffect, useMemo, useState } from "react";
import { invoker } from "./engine";
import { KNOWN_ACTIONS, kindOf, type StepKind } from "./flow";

type Ask = { state: "asking" } | { state: "asked"; seen: string[] } | { state: "mute"; why: string };

/** What each family is for, in the words the canvas uses for its own nodes. */
const FAMILY: Record<StepKind, string> = {
  trigger: "where a run starts",
  engine: "hands work to a model",
  check: "asks something and answers yes or no",
  gesture: "reaches out and does something",
  human: "leaves the work to whoever is there",
  deposit: "reads or writes what Sailor remembers",
  subflow: "runs another flow",
  wait: "holds until something happens",
  branch: "picks a way",
};

export function AbilitiesScreen({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no engine to ask" });
      return;
    }
    const invoke = invoker();
    if (!invoke) return;
    invoke<string[]>("engine_actions").then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  const groups = useMemo(() => {
    if (ask.state !== "asked") return [];
    const byKind = new Map<StepKind, string[]>();
    for (const action of ask.seen) {
      const kind = kindOf(action);
      const list = byKind.get(kind);
      if (list) list.push(action);
      else byKind.set(kind, [action]);
    }
    return [...byKind.entries()];
  }, [ask]);

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot ask the engine: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Asking the engine what it registers…</p></div>;
  }

  /* AN ACTION THE ENGINE HAS AND THE CANVAS CANNOT DRAW would land in the
     `check` family by the fallback and look like an ordinary one. Named here,
     it reads as what it is: something usable in a flow file that this window
     has no node for yet. */
  const unknownToCanvas = ask.seen.filter((action) => !KNOWN_ACTIONS.includes(action));

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">What Sailor can do</h2>
        <span className="now__count">{ask.seen.length}</span>
      </header>
      <p className="now__mute">
        Asked of the running engine, not read from a list kept here. These are the words a
        flow file may use for a step’s <code>action</code>.
      </p>

      {unknownToCanvas.length > 0 && (
        <p className="now__mute" data-bad>
          {unknownToCanvas.length} of them have no node on the canvas yet: {unknownToCanvas.join(" · ")}.
          A flow can still use them; the board will draw them as ordinary checks.
        </p>
      )}

      {groups.map(([kind, actions]) => (
        <section className="panel__block" key={kind}>
          <div className="panel__title">
            {kind} <span className="rail__note">{FAMILY[kind]}</span>
          </div>
          <ul className="abilities">
            {actions.map((action) => (
              <li className="abilities__one" key={action}>
                <code>{action}</code>
                {!KNOWN_ACTIONS.includes(action) && (
                  <span className="now__why" data-bad>no node for this one</span>
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
