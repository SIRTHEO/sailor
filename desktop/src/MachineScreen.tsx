/**
 * What this machine has, where it was looked for, and what would not read.
 * **A LIST THAT DOES NOT SAY WHERE IT SEARCHED CANNOT BE CONTRADICTED**: whoever
 * knows they have a tool cannot tell a missing one from an unopened folder.
 */
import { useEffect, useMemo, useState } from "react";
import { PRESENCE_WORD, sweep, type Sweep, type Tool } from "./machine";

type Ask = { state: "asking" } | { state: "asked"; seen: Sweep } | { state: "mute"; why: string };

/** Families in the order they matter, then whatever else a descriptor declares. */
function byFamily(tools: Tool[]): [string, Tool[]][] {
  const groups = new Map<string, Tool[]>();
  for (const tool of tools) {
    const list = groups.get(tool.kind);
    if (list) list.push(tool);
    else groups.set(tool.kind, [tool]);
  }
  return [...groups.entries()];
}

export function MachineScreen({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no machine to sweep" });
      return;
    }
    sweep().then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  const groups = useMemo(
    () => (ask.state === "asked" ? byFamily(ask.seen.tools) : []),
    [ask],
  );

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot sweep this machine: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Asking each tool what it is…</p></div>;
  }

  const here = ask.seen.tools.filter((tool) => tool.presence === "present").length;
  const unknown = ask.seen.tools.filter((tool) => tool.presence === "undetermined").length;

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">This machine</h2>
        <span className="now__count">{here} of {ask.seen.tools.length}</span>
      </header>
      <p className="now__mute">
        Each was asked for itself, just now: a tool installed while this window is open
        shows up on the next look.
      </p>

      {/* A FAULT IN THE LIST IS NOT A MISSING TOOL, and it goes first: while it
          stands, everything below it is a list drawn from an incomplete set of
          instructions, and nobody should read the rest without knowing that. */}
      {ask.seen.problems.length > 0 && (
        <section className="panel__block">
          <div className="panel__title">Lines of the list that would not read</div>
          <table className="now__table">
            <thead><tr><th>where</th><th>which</th><th>why</th></tr></thead>
            <tbody>
              {ask.seen.problems.map((bad) => (
                <tr key={`${bad.source}/${bad.about}`}>
                  <td className="now__path">{bad.source}</td>
                  <td className="now__entity">{bad.about}</td>
                  <td data-bad>{bad.reason}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {groups.map(([family, tools]) => {
        const shown = showAll ? tools : tools.filter((tool) => tool.presence !== "absent");
        return (
          <section className="panel__block" key={family}>
            <div className="panel__title">
              {family} <span className="rail__note">{tools.filter((t) => t.presence === "present").length} of {tools.length} here</span>
            </div>
            {shown.length === 0 ? (
              <p className="now__empty">Nothing of this kind is on the machine.</p>
            ) : (
              <table className="now__table now__table--four">
                <thead><tr><th>tool</th><th>where</th><th>version</th><th>state</th></tr></thead>
                <tbody>
                  {shown.map((tool) => (
                    <tr key={tool.id} data-here={tool.presence === "present" || undefined}>
                      <td className="now__entity">
                        {tool.name}
                        {/* THE DESCRIPTOR THAT RECOGNISED IT is the address for
                            holding a wrong row to account. A list nobody can
                            question does not get corrected. */}
                        <div className="now__why">from the descriptor «{tool.descriptor}»</div>
                      </td>
                      <td className="now__path">{tool.path ?? "—"}</td>
                      {/* A VERSION NOT OBTAINED STAYS ABSENT: a dash is not a
                          number that happens to look old. */}
                      <td>{tool.version ?? "not stated"}</td>
                      <td data-state={tool.presence}>
                        {PRESENCE_WORD[tool.presence]}
                        <div className="now__why">{tool.reason}</div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        );
      })}

      <section className="panel__block">
        <div className="panel__title">Where it looked</div>
        <p className="now__why">
          {ask.seen.looked_in.length === 0
            ? "Nowhere: no directory was searched, which is itself the answer."
            : ask.seen.looked_in.join(" · ")}
        </p>
        {unknown > 0 && (
          <p className="now__mute" data-bad>
            {unknown} could not be checked at all. Those are not missing — the check itself
            did not run, and the reason is on each row.
          </p>
        )}
        <div className="now__new">
          <label className="now__toggle">
            <input type="checkbox" checked={showAll} onChange={(event) => setShowAll(event.target.checked)} />
            also show what is not here
          </label>
        </div>
      </section>
    </div>
  );
}
