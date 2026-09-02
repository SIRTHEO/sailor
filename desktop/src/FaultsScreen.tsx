/**
 * What has broken, and what would have prevented it. **THE LAST COLUMN IS WHY
 * THIS EXISTS**: without «what would have caught it» this is a diary, so an
 * entry missing it is marked instead of drawn as though it were complete.
 */
import { useCallback, useEffect, useState } from "react";
import { register, setStatus, STATUS_WORDS, type Entry, type Register as Book } from "./register";

type Ask = { state: "asking" } | { state: "asked"; seen: Book } | { state: "mute"; why: string };

export function FaultsScreen({ native }: { native: boolean }) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [openOnly, setOpenOnly] = useState(true);
  const [busy, setBusy] = useState<number | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  const read = useCallback(() => {
    register().then(
      (seen) => setAsk({ state: "asked", seen }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      setAsk({ state: "mute", why: "outside the desktop shell there is no register to read" });
      return;
    }
    read();
  }, [native, read]);

  const move = useCallback(
    (number: number, status: string) => {
      setBusy(number);
      setFailed(null);
      setStatus(number, status).then(
        () => { setBusy(null); read(); },
        (error) => { setBusy(null); setFailed(String(error)); },
      );
    },
    [read],
  );

  if (ask.state === "mute") {
    return <div className="now"><p className="now__mute">I cannot read the register: {ask.why}</p></div>;
  }
  if (ask.state === "asking") {
    return <div className="now"><p className="now__mute">Reading the register…</p></div>;
  }

  const shown: Entry[] = openOnly
    // `unrecognised` is kept in the open view on purpose: it is not closed, and
    // hiding it would be the quiet subtraction the fourth answer exists against.
    ? ask.seen.entries.filter((entry) => entry.standing !== "closed")
    : ask.seen.entries;

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Faults</h2>
        <span className="now__count">{ask.seen.still_open} open</span>
      </header>
      <p className="now__mute">The register is at <span className="now__path">{ask.seen.path}</span></p>
      {failed !== null && <p className="now__mute" data-bad>That did not work: {failed}</p>}

      <div className="now__new">
        <label className="now__toggle">
          <input type="checkbox" checked={openOnly} onChange={(event) => setOpenOnly(event.target.checked)} />
          only what is not closed
        </label>
      </div>

      {shown.length === 0 ? (
        <p className="now__empty">Nothing on record here.</p>
      ) : (
        shown.map((entry) => (
          <section className="panel__block" key={entry.number}>
            <div className="panel__title">
              {entry.number}. {entry.what_happened}
              <span className="rail__note" data-state={entry.standing}> {entry.standing}</span>
            </div>
            <dl className="now__kv">
              <dt>on</dt>
              <dd>{entry.happened_on}</dd>
              <dt>how it showed</dt>
              <dd>{entry.how_it_showed}</dd>
              <dt>what would prevent it</dt>
              <dd>
                {entry.what_would_prevent.trim() === "" ? (
                  <span data-bad>nothing written: this entry is not finished</span>
                ) : (
                  entry.what_would_prevent
                )}
              </dd>
              <dt>status</dt>
              <dd>
                {entry.status}
                {entry.standing === "unrecognised" && (
                  // NOT A FAULT OF THE FAULT: the register did not understand
                  // the wording, and saying so is what keeps it from being
                  // counted as closed by accident.
                  <div className="now__why" data-bad>
                    the register does not recognise this wording, so it counts as neither open nor closed
                  </div>
                )}
              </dd>
            </dl>
            <div className="now__new">
              {(Object.entries(STATUS_WORDS) as [string, string][]).map(([name, prose]) => (
                entry.standing !== name && (
                  <button
                    key={name}
                    type="button"
                    className="rail__all"
                    disabled={busy !== null}
                    onClick={() => move(entry.number, prose)}
                  >
                    mark {name}
                  </button>
                )
              ))}
            </div>
          </section>
        ))
      )}
    </div>
  );
}
