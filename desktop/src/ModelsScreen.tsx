/**
 * Which model the work runs on, and which one may be chosen instead. **THE
 * SETTING COMES BEFORE THE PRICE LIST**, and it says «effective», never
 * «used»: `Choice` is the configuration once the free-only rule is applied,
 * and what a run really called is the ledger's.
 */
import { useCallback, useEffect, useState } from "react";
import {
  catalogue,
  perMillion,
  setModel,
  type Catalogue,
  type Choice,
  type Priced,
} from "./quota";
import type { Ask } from "./QuotaScreen";

/** How many rows the catalogue draws before it says it stopped. */
const AT_MOST = 60;

/** The model a choice really lands on, resolved to a name the catalogue gives. */
export function named(id: string | null, models: Priced[]): string | null {
  if (id === null) return null;
  const found = models.find((model) => model.id === id);
  return found === undefined ? id : found.name;
}

/** For each model id, the kinds it is effective for. Empty for the rest. */
export function effectiveFor(choices: Choice[]): Map<string, string[]> {
  const kinds = new Map<string, string[]>();
  for (const choice of choices) {
    if (choice.in_force === null) continue;
    kinds.set(choice.in_force, [...(kinds.get(choice.in_force) ?? []), choice.kind]);
  }
  return kinds;
}

/**
 * The rows to draw, effective ones first. **TRUNCATION MUST NOT HIDE A MARK**:
 * cut at sixty in catalogue order, the model a kind runs on could fall past
 * the cut and the list would show no model marked at all.
 */
export function inOrder(models: Priced[], effective: Map<string, string[]>): Priced[] {
  const marked = models.filter((model) => effective.has(model.id));
  return [...marked, ...models.filter((model) => !effective.has(model.id))];
}

function Settings({ choices, models }: { choices: Choice[]; models: Priced[] }) {
  return (
    <table className="now__table">
      <thead><tr><th>work</th><th>effective model</th></tr></thead>
      <tbody>
        {choices.map((choice) => (
          <tr key={choice.kind}>
            <td className="now__entity">{choice.kind}</td>
            <td>
              {choice.in_force === null ? (
                <span data-bad>No free model matched: this work has nothing to run on.</span>
              ) : (
                <>
                  {named(choice.in_force, models)}
                  <div className="now__why">{choice.in_force}</div>
                </>
              )}
              {/* WHAT WAS ASKED FOR AND WHAT RUNS ARE NOT ALWAYS THE SAME. A
                  saved choice that no longer points at a free model is quietly
                  overruled, and a screen showing the wish explains nothing. */}
              {choice.chosen !== null && choice.chosen !== choice.in_force && (
                <div className="now__why" data-bad>
                  configured: {named(choice.chosen, models)} ({choice.chosen}) — the free-only rule overrules it
                </div>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function ModelsScreen({ native, onQuota }: { native: boolean; onQuota?: () => void }) {
  const [book, setBook] = useState<Ask<Catalogue>>({ state: "asking" });
  const [look, setLook] = useState("");
  const [withPaid, setWithPaid] = useState(false);
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
      setBook({ state: "mute", why: "outside the desktop shell there is no engine to ask" });
      return;
    }
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

  // ONE READ CARRIES BOTH BLOCKS, so one failure must not read as «nothing is
  // configured»: what is unknown says it is unknown, and offers the read again.
  if (book.state !== "asked") {
    return (
      <div className="now">
        <header className="now__head"><h2 className="now__title">Models</h2></header>
        {book.state === "asking" ? (
          <p className="now__mute">Asking for the settings and the catalogue…</p>
        ) : (
          <>
            <p className="now__mute" data-bad>
              Neither the settings nor the catalogue could be read: {book.why}
            </p>
            <button type="button" className="rail__all" onClick={readBook} disabled={!native}>ask again</button>
          </>
        )}
      </div>
    );
  }

  const { models, choices } = book.seen;
  const effective = effectiveFor(choices);
  const free = models.filter((model) => model.free);
  const needle = look.trim().toLowerCase();
  const shown = inOrder(withPaid ? models : free, effective).filter(
    (model) =>
      needle === "" || model.id.toLowerCase().includes(needle) || model.name.toLowerCase().includes(needle),
  );

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Models</h2>
      </header>

      <section className="panel__block">
        <div className="panel__title">What each kind of work runs on</div>
        <Settings choices={choices} models={models} />
        <p className="now__why">
          These are the settings in force, not what a run really called: what was called is
          in the ledger, under Runs.
          {onQuota && (
            <>
              {" "}
              <button type="button" className="chip__button" onClick={onQuota}>
                How much of each provider’s quota is gone
              </button>{" "}
              is a separate question: no window here says whether one of these models is reachable.
            </>
          )}
        </p>
      </section>

      <section className="panel__block">
        <div className="panel__title">
          Choose a free model{" "}
          <span className="rail__note">
            {free.length} free to choose · {models.length} in the catalogue
          </span>
        </div>
        {failed !== null && <p className="now__mute" data-bad>That did not work: {failed}</p>}
        <div className="now__new">
          <input
            className="now__field"
            value={look}
            placeholder="search by name or id"
            onChange={(event) => setLook(event.target.value)}
          />
          <label className="now__toggle">
            <input type="checkbox" checked={withPaid} onChange={(event) => setWithPaid(event.target.checked)} />
            include paid models for reference
          </label>
        </div>
        {shown.length === 0 ? (
          // AN EMPTY SEARCH OVER THE FREE ONES IS NOT AN EMPTY CATALOGUE, and a
          // person who reads it as one concludes the model is not there at all.
          <p className="now__empty">
            {withPaid
              ? "Nothing in the catalogue matches that."
              : "No free model matches that. Paid models are hidden: tick the box to look at them."}
          </p>
        ) : (
          <table className="now__table">
            <thead>
              <tr><th>model</th><th>in</th><th>out</th><th>context</th><th /></tr>
            </thead>
            <tbody>
              {shown.slice(0, AT_MOST).map((model) => (
                <tr key={model.id}>
                  <td className="now__entity">
                    {model.name}
                    {(effective.get(model.id) ?? []).map((kind) => (
                      <span className="now__badge" key={kind}>effective for {kind}</span>
                    ))}
                    <div className="now__why">{model.id} · {model.modalities.join(" · ")}</div>
                  </td>
                  <td>{perMillion(model.price_in)}</td>
                  <td>{perMillion(model.price_out)}</td>
                  <td>{model.context_length === null ? "not stated" : model.context_length.toLocaleString("en-US")}</td>
                  <td>
                    {/* ONLY THE FREE ONES CAN BE CHOSEN, and that is the
                        engine's rule, not a decoration: offering the button on
                        a paid model would put the refusal after the click. The
                        row says why instead, or the absence explains nothing. */}
                    {model.free ? (
                      <button
                        type="button"
                        className="rail__all"
                        disabled={busy}
                        onClick={() => choose("default", model.id)}
                      >
                        use for default
                      </button>
                    ) : (
                      <span className="now__why">paid: the free-only rule refuses it</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {shown.length > AT_MOST && (
          // NO SILENT TRUNCATION: a list that stops without saying so reads as
          // a list that ended.
          <p className="now__mute">Showing {AT_MOST} of {shown.length}. Narrow the search to see the rest.</p>
        )}
      </section>
    </div>
  );
}
