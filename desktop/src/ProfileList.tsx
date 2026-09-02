/**
 * The command lines Sailor knows, and the profiles each one has. **THE NOTES
 * ARE THE POINT**: every claim here — «no native profiles», «the home moves
 * with this variable» — carries how it was found out, because a screen that
 * asserts without evidence is the thing this product is against.
 */
import { useCallback, useEffect, useState } from "react";
import {
  commandLines,
  create,
  rows,
  switchTo,
  type Access,
  type CommandLine,
  type Row,
} from "./profiles";

type Ask =
  | { state: "asking" }
  | { state: "asked"; clis: CommandLine[]; profiles: Row[] }
  | { state: "mute"; why: string };

/** What each verdict is called on screen, and nothing is called nothing. */
const ACCESS_WORD: Record<Access, string> = {
  yes: "signed in",
  no: "not signed in",
  "not known": "nobody could look",
  "home does not move": "changes nothing",
};

function mechanism(cli: CommandLine): string {
  if (cli.home_mechanism === "variable") return `moves with ${cli.home_detail}`;
  if (cli.home_mechanism === "symlink") return `swaps ${cli.home_detail}`;
  return "its home does not move";
}

export function ProfileList({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [busy, setBusy] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const [name, setName] = useState("");

  const read = useCallback(() => {
    Promise.all([commandLines(), rows()]).then(
      ([clis, profiles]) => setAsk({ state: "asked", clis, profiles }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no command line to ask" });
      return;
    }
    read();
  }, [native, read]);

  /** A gesture that touches the disk, then re-reads: the screen never guesses
      what changed, it asks again. */
  const act = useCallback(
    (key: string, work: Promise<void>) => {
      setBusy(key);
      setFailed(null);
      work.then(
        () => { setBusy(null); setNaming(null); setName(""); read(); },
        (error) => { setBusy(null); setFailed(String(error)); },
      );
    },
    [read],
  );

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot read the profiles: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Asking each command line…</p></div>;
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Profiles</h2>
        <span className="now__count">{ask.profiles.length}</span>
      </header>
      {/* WHAT THIS SCREEN COST TO DRAW, said out loud: it ran one command per
          profile. They read a local file and call no model, but somebody
          watching their machine deserves to know what opened it. */}
      <p className="now__mute">
        Each row was asked of the real command line, inside that profile’s own home.
      </p>
      {failed !== null && <p className="now__mute" data-bad>That did not work: {failed}</p>}

      {ask.clis.map((cli) => {
        const mine = ask.profiles.filter((row) => row.cli_id === cli.id);
        return (
          <section className="panel__block" key={cli.id}>
            {/* THE NAME ONCE, THE COMMAND ONCE. Both in the title's own
                letterforms read as the same thing said twice, and the second
                one wrapped onto its own line looking like a heading. */}
            <div className="panel__title">{cli.display_name}</div>
            <div className="now__command">{cli.executable}</div>

            <dl className="now__kv">
              <dt>home</dt>
              <dd>
                {mechanism(cli)}
                <div className="now__why">{cli.home_note}</div>
              </dd>
              <dt>own profiles</dt>
              <dd>
                {cli.native_profiles}
                <div className="now__why">{cli.native_profiles_note}</div>
              </dd>
            </dl>

            {/* A COMMAND LINE WHOSE HOME DOES NOT MOVE TAKES PROFILES AND USES
                NONE OF THEM. Letting the gesture look the same here would be
                the window promising something the engine cannot do. */}
            {cli.home_mechanism === "none" && (
              <p className="now__mute" data-bad>
                Profiles made here would all start it in the same place. Nothing switches.
              </p>
            )}

            {mine.length === 0 ? (
              cli.home_mechanism !== "none" && (
                <p className="now__empty">No profile yet for {cli.display_name}.</p>
              )
            ) : (
              <table className="now__table">
                <thead>
                  <tr><th>profile</th><th>home</th><th>access</th><th /></tr>
                </thead>
                <tbody>
                  {mine.map((row) => (
                    <tr key={`${row.cli_id}/${row.name}`} data-here={row.active || undefined}>
                      <td className="now__entity">
                        {row.name}
                        {row.active && <span className="rail__note"> — in force</span>}
                      </td>
                      <td className="now__path">{row.home_dir}</td>
                      {/* THE VERDICT AND THE WORDS BEHIND IT, TOGETHER. «not
                          signed in» sends you to log in; «nobody could look»
                          sends you to the engine, and telling them apart is
                          the whole reason the sentence travels. */}
                      <td data-access={row.access}>
                        {ACCESS_WORD[row.access]}
                        <div className="now__why">{row.said}</div>
                      </td>
                      <td>
                        {!row.active && (
                          <button
                            type="button"
                            className="rail__all"
                            disabled={busy !== null}
                            onClick={() => act(`${row.cli_id}/${row.name}`, switchTo(row.cli_id, row.name))}
                          >
                            {busy === `${row.cli_id}/${row.name}` ? "switching…" : "use this one"}
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            {cli.home_mechanism === "none" ? null : naming === cli.id ? (
              <form
                className="now__new"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (name.trim() !== "") act(`new/${cli.id}`, create(cli.id, name.trim()));
                }}
              >
                <input
                  className="now__field"
                  autoFocus
                  value={name}
                  placeholder="a name for it"
                  onChange={(event) => setName(event.target.value)}
                />
                <button type="submit" className="rail__all" disabled={busy !== null}>
                  make it
                </button>
                <button type="button" className="rail__all" onClick={() => setNaming(null)}>
                  never mind
                </button>
              </form>
            ) : (
              <button type="button" className="rail__new" onClick={() => { setNaming(cli.id); setName(""); }}>
                + New profile
              </button>
            )}
          </section>
        );
      })}
    </div>
  );
}
