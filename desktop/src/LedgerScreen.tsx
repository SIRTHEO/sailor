/**
 * What the ledger holds, asked the questions nobody was asking it. Sailor
 * records everything and never went back to read it: the window showed the
 * history of runs and nothing else.
 */
import { useEffect, useState } from "react";
import { held, type Held } from "./held";

type Ask = { state: "asking" } | { state: "asked"; seen: Held } | { state: "mute"; why: string };

function ago(at: number, now: number): string {
  const gap = Math.max(0, now - at);
  if (gap < 60) return "just now";
  if (gap < 3600) return `${Math.floor(gap / 60)} min ago`;
  if (gap < 86_400) return `${Math.floor(gap / 3600)} h ago`;
  return `${Math.floor(gap / 86_400)} d ago`;
}

export function LedgerScreen({ native, now }: { native: boolean; now: number }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no ledger to read" });
      return;
    }
    held().then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [native]);

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot read the ledger: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Reading the ledger…</p></div>;
  }

  const seen = ask.seen;

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">The ledger</h2>
        <span className="now__count">{seen.runs} runs</span>
      </header>
      <p className="now__mute">It is at <span className="now__path">{seen.directory}</span></p>

      {/* «NOT CREATED YET» AND «EMPTY» READ IDENTICALLY FROM A COUNT, and the
          first is the normal state of a fresh install. */}
      {!seen.exists ? (
        <p className="now__empty">
          There is no ledger there yet. It is written the first time a flow runs — until
          then there is nothing to read, which is not the same as nothing having happened.
        </p>
      ) : (
        <>
          {/* WHAT IS STILL STANDING GOES FIRST: it is the only part of this
              screen about right now rather than about the past. */}
          <section className="panel__block">
            <div className="panel__title">
              Processes the ledger never saw end
              <span className="rail__note"> {seen.leftovers.length}</span>
            </div>
            {seen.leftovers.length === 0 ? (
              <p className="now__empty">None: every process it recorded starting, it recorded ending.</p>
            ) : (
              <table className="now__table now__table--four">
                <thead><tr><th>process</th><th>command</th><th>port</th><th>pid</th></tr></thead>
                <tbody>
                  {seen.leftovers.map((one) => (
                    <tr key={one.process_id}>
                      <td className="now__entity">{one.process_id}
                        <div className="now__why">{one.working_directory}</div>
                      </td>
                      <td className="now__path">{one.command}</td>
                      <td>{one.port === null ? "not held" : one.port}</td>
                      {/* THE RECORD AND THE PROCESS ARE TWO FACTS. A record
                          left open with a dead pid is a ledger to tidy; one
                          with a live pid is a process to deal with. */}
                      <td data-state={one.alive ? "undetermined" : "absent"}>
                        {one.pid} · {one.alive ? "still running" : "gone"}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="panel__block">
            <div className="panel__title">Runs that never closed<span className="rail__note"> {seen.unfinished.length}</span></div>
            {seen.unfinished.length === 0 ? (
              <p className="now__empty">None open.</p>
            ) : (
              <table className="now__table">
                <thead><tr><th>flow</th><th>steps still open</th><th>oldest step</th></tr></thead>
                <tbody>
                  {seen.unfinished.map((run) => (
                    <tr key={run.run_id}>
                      <td className="now__entity">{run.entity === "" ? "unnamed" : run.entity}
                        <div className="now__why">{run.run_id}</div>
                      </td>
                      <td>{run.open_steps}</td>
                      <td>{ago(run.oldest_started_at, now)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          {seen.waiting.length > 0 && (
            <section className="panel__block">
              <div className="panel__title">Waiting for somebody<span className="rail__note"> {seen.waiting.length}</span></div>
              <table className="now__table">
                <thead><tr><th>flow</th><th>since</th></tr></thead>
                <tbody>
                  {seen.waiting.map((run) => (
                    <tr key={run.run_id}>
                      <td className="now__entity">{run.entity === "" ? "unnamed" : run.entity}</td>
                      <td>{ago(run.waiting_since, now)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}

          <section className="panel__block">
            <div className="panel__title">How the last 50 runs broke</div>
            {seen.failures.length === 0 ? (
              <p className="now__empty">Nothing broke in them.</p>
            ) : (
              <table className="now__table">
                <thead><tr><th>class</th><th>failures</th><th>runs hit</th></tr></thead>
                <tbody>
                  {seen.failures.map((one) => (
                    <tr key={one.class ?? " "}>
                      {/* A MISSING CLASS IS NOT A CLASS CALLED «UNKNOWN»: the
                          engine could not classify that failure, and counting
                          it among a named class would invent a pattern. */}
                      <td className="now__entity">
                        {one.class ?? <span data-bad>not classified</span>}
                      </td>
                      <td>{one.failures}</td>
                      <td>{one.runs_affected}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="panel__block">
            <div className="panel__title">What flows have kept<span className="rail__note"> {seen.kept.length}</span></div>
            {seen.kept.length === 0 ? (
              <p className="now__empty">
                Nothing in the collections Sailor writes. A flow can name its own, and one
                nobody here knows about would not show up in this list.
              </p>
            ) : (
              <table className="now__table">
                <thead><tr><th>collection</th><th>key</th></tr></thead>
                <tbody>
                  {seen.kept.map((one) => (
                    <tr key={`${one.collection}/${one.key}`}>
                      <td className="now__entity">{one.collection}</td>
                      <td className="now__path">{one.key}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="panel__block">
            <div className="panel__title">The inventory over time</div>
            <dl className="now__kv">
              <dt>here now</dt>
              <dd>{seen.inventory_present}</dd>
              <dt>seen once and gone</dt>
              <dd>{seen.inventory_gone}</dd>
            </dl>
          </section>
        </>
      )}
    </div>
  );
}
