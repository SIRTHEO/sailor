/**
 * How much of a quota is gone, which model is in force, and what the catalogue
 * costs. **THE NUMBER SAYS SPENT, NEVER LEFT**: the provider declares what is
 * gone, and on a window that never states its ceiling the rest would be a
 * subtraction we invented.
 */
import { useCallback, useEffect, useState } from "react";
import {
  catalogue,
  perMillion,
  quota,
  setModel,
  windowName,
  type Catalogue,
  type Choice,
  type Priced,
  type Window as QuotaWindow,
} from "./quota";

type Ask<T> = { state: "asking" } | { state: "asked"; seen: T } | { state: "mute"; why: string };

/** A bar for a fraction already spent. Full is not a failure — it is a fact. */
function Spent({ fraction }: { fraction: number }) {
  const percent = Math.max(0, Math.min(1, fraction)) * 100;
  return (
    <div className="quota__bar" data-full={percent >= 100 || undefined}>
      <div className="quota__fill" style={{ width: `${percent}%` }} />
    </div>
  );
}

export function QuotaScreen({ native, now }: { native: boolean; now: number }) {
  const [windows, setWindows] = useState<Ask<QuotaWindow[]>>({ state: "asking" });
  const [book, setBook] = useState<Ask<Catalogue>>({ state: "asking" });
  const [look, setLook] = useState("");
  const [freeOnly, setFreeOnly] = useState(false);
  const [failed, setFailed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const readBook = useCallback(() => {
    setBook({ state: "asking" });
    catalogue().then(
      (seen) => setBook({ state: "asked", seen }),
      (error) => setBook({ state: "mute", why: String(error) }),
    );
  }, []);

  useEffect(() => {
    if (!native) {
      const why = "outside the desktop shell there is no engine to ask";
      setWindows({ state: "mute", why });
      setBook({ state: "mute", why });
      return;
    }
    quota().then(
      (seen) => setWindows({ state: "asked", seen }),
      (error) => setWindows({ state: "mute", why: String(error) }),
    );
    readBook();
  }, [native, readBook]);

  const choose = useCallback(
    (kind: string, id: string) => {
      setBusy(true);
      setFailed(null);
      setModel(kind, id).then(
        () => { setBusy(false); readBook(); },
        (error) => { setBusy(false); setFailed(String(error)); },
      );
    },
    [readBook],
  );

  const models: Priced[] = book.state === "asked" ? book.seen.models : [];
  const choices: Choice[] = book.state === "asked" ? book.seen.choices : [];
  const needle = look.trim().toLowerCase();
  const shown = models.filter(
    (model) =>
      (!freeOnly || model.free) &&
      (needle === "" || model.id.toLowerCase().includes(needle) || model.name.toLowerCase().includes(needle)),
  );

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Quota and models</h2>
      </header>

      <section className="panel__block">
        <div className="panel__title">Quota already spent</div>
        {windows.state === "mute" ? (
          // A CHANNEL THAT DOES NOT ANSWER IS NEVER A QUOTA OF ZERO, which is
          // the reassuring direction. The engine's own words say what to do.
          <p className="now__mute" data-bad>I could not read it: {windows.why}</p>
        ) : windows.state === "asking" ? (
          <p className="now__mute">Asking…</p>
        ) : windows.seen.length === 0 ? (
          <p className="now__empty">The engine reports no window at all.</p>
        ) : (
          <>
            <table className="now__table">
              <thead>
                <tr><th>window</th><th>spent</th><th>resets</th><th>read</th></tr>
              </thead>
              <tbody>
                {windows.seen.map((one) => (
                  <tr key={`${one.engine}/${one.unit}`}>
                    <td className="now__entity">{windowName(one.unit)}
                      <div className="now__why">{one.engine}</div>
                    </td>
                    <td>
                      {(one.spent_fraction * 100).toFixed(1)}%
                      <Spent fraction={one.spent_fraction} />
                    </td>
                    <td>{one.resets_at ?? "not stated"}</td>
                    {/* A QUOTA AGES. Without the instant it was read at, this
                        cannot be told from yesterday's reading. */}
                    <td>{secondsAgo(one.observed_at, now)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="now__why">
              This is the whole person’s quota — every session, the terminal beside this
              one, yesterday’s job in the same window. It is not the cost of a run.
            </p>
          </>
        )}
      </section>

      <section className="panel__block">
        <div className="panel__title">What each kind of work runs on</div>
        {failed !== null && <p className="now__mute" data-bad>That did not work: {failed}</p>}
        {book.state === "mute" ? (
          <p className="now__mute" data-bad>I could not read the catalogue: {book.why}</p>
        ) : book.state === "asking" ? (
          <p className="now__mute">Downloading the catalogue…</p>
        ) : (
          <table className="now__table">
            <thead><tr><th>kind</th><th>in force</th><th>configured</th></tr></thead>
            <tbody>
              {choices.map((choice) => (
                <tr key={choice.kind}>
                  <td className="now__entity">{choice.kind}</td>
                  <td>{choice.in_force ?? "nothing: no free model matched"}</td>
                  {/* WHAT WAS ASKED FOR AND WHAT RUNS ARE NOT ALWAYS THE SAME.
                      A saved choice that no longer points at a free model is
                      quietly overruled by the engine, and a screen showing only
                      the wish would explain nothing when the run differs. */}
                  <td>
                    {choice.chosen ?? "—"}
                    {choice.chosen !== null && choice.chosen !== choice.in_force && (
                      <div className="now__why" data-bad>
                        configured but not in force: the free-only rule overrules it
                      </div>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      {book.state === "asked" && (
        <section className="panel__block">
          <div className="panel__title">The catalogue <span className="rail__note">{models.length} models</span></div>
          <div className="now__new">
            <input
              className="now__field"
              value={look}
              placeholder="search by name or id"
              onChange={(event) => setLook(event.target.value)}
            />
            <label className="now__toggle">
              <input type="checkbox" checked={freeOnly} onChange={(event) => setFreeOnly(event.target.checked)} />
              only free
            </label>
          </div>
          {shown.length === 0 ? (
            <p className="now__empty">Nothing in the catalogue matches that.</p>
          ) : (
            <table className="now__table">
              <thead>
                <tr><th>model</th><th>in</th><th>out</th><th>context</th><th /></tr>
              </thead>
              <tbody>
                {shown.slice(0, 60).map((model) => (
                  <tr key={model.id}>
                    <td className="now__entity">
                      {model.name}
                      <div className="now__why">{model.id} · {model.modalities.join(" · ")}</div>
                    </td>
                    <td>{perMillion(model.price_in)}</td>
                    <td>{perMillion(model.price_out)}</td>
                    <td>{model.context_length === null ? "not stated" : model.context_length.toLocaleString("en-US")}</td>
                    <td>
                      {/* ONLY THE FREE ONES CAN BE CHOSEN, and that is the
                          engine's rule, not a decoration: offering the button
                          on a paid model would put the refusal after the click
                          instead of before it. */}
                      {model.free && (
                        <button
                          type="button"
                          className="rail__all"
                          disabled={busy}
                          onClick={() => choose("default", model.id)}
                        >
                          use for default
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {shown.length > 60 && (
            // NO SILENT TRUNCATION: a list that stops without saying so reads
            // as a list that ended.
            <p className="now__mute">Showing 60 of {shown.length}. Narrow the search to see the rest.</p>
          )}
        </section>
      )}
    </div>
  );
}

/** How long ago a reading was taken, in the words a person uses. */
function secondsAgo(at: number, now: number): string {
  const gap = Math.max(0, now - at);
  if (gap < 60) return "just now";
  if (gap < 3600) return `${Math.floor(gap / 60)} min ago`;
  if (gap < 86_400) return `${Math.floor(gap / 3600)} h ago`;
  return `${Math.floor(gap / 86_400)} d ago`;
}
