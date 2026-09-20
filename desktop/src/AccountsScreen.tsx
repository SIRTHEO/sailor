/**
 * The accounts, one card each: whose it is, what it has spent of each window,
 * and what it did in the last day. **THE TWO WINDOWS ARE DRAWN APART**, named
 * by their own length — a session and a week called the same thing was the
 * whole reason this screen exists.
 */
import { useCallback, useEffect, useState } from "react";
import { accounts, accountName, type Account, type Allowance, type Standing } from "./accounts";
import { BrandMark } from "./BrandMark";

type Ask = { state: "asking" } | { state: "asked"; value: Account[] } | { state: "mute"; why: string };

/** A day: long enough to hold a night's work, short enough to still be today. */
const A_DAY = 24;

const STANDING_WORD: Record<Standing, string> = {
  ready: "ready",
  ran_out: "out of quota",
  shut: "cannot be used",
  unknown: "nobody could tell",
};

/** A bar for a fraction already spent. Full is not a failure — it is a fact. */
function Spent({ fraction }: { fraction: number }) {
  const percent = Math.max(0, Math.min(1, fraction)) * 100;
  return (
    <div className="quota__bar" data-full={percent >= 100 || undefined}>
      <div className="quota__fill" style={{ width: `${percent}%` }} />
    </div>
  );
}

function Window({ one }: { one: Allowance }) {
  return (
    <div className="account__window">
      <div className="account__windowhead">
        <span className="account__windowname">{one.word}</span>
        <span className="account__windowspent">{(one.used_fraction * 100).toFixed(0)}%</span>
      </div>
      <Spent fraction={one.used_fraction} />
      {one.resets_at !== null && <div className="now__why">back at {one.resets_at}</div>}
    </div>
  );
}

/** What the account did in its own home, in the shape a person scans. */
function Did({ one }: { one: Account }) {
  const worked = one.worked;
  if (worked === null || worked.calls === 0) {
    return <p className="now__why">Nothing in the last day.</p>;
  }
  const models = [...worked.by_model].sort((a, b) => b.input + b.output - (a.input + a.output));
  return (
    <>
      <p className="account__did">
        {worked.calls} calls over {worked.sessions} {worked.sessions === 1 ? "session" : "sessions"}
      </p>
      <ul className="account__models">
        {models.slice(0, 4).map((model) => (
          <li key={model.model}>
            <span className="account__model">{model.model}</span>
            <span className="now__why">{tokens(model.input + model.output)}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

export function tokens(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M tokens`;
  if (count >= 1_000) return `${Math.round(count / 1_000)}k tokens`;
  return `${count} tokens`;
}

export function AccountsScreen({ native, readings }: { native: boolean; readings?: Ask }) {
  const [own, setOwn] = useState<Ask>({ state: "asking" });
  const [reading, setReading] = useState(false);
  const seen = readings ?? own;

  const ask = useCallback(
    (withQuota: boolean) => {
      if (withQuota) setReading(true);
      accounts(A_DAY, withQuota).then(
        (value) => {
          setOwn({ state: "asked", value });
          setReading(false);
        },
        (error) => {
          setOwn({ state: "mute", why: String(error) });
          setReading(false);
        },
      );
    },
    [],
  );

  useEffect(() => {
    if (!native) {
      setOwn({ state: "mute", why: "outside the desktop shell there is no engine to ask" });
      return;
    }
    // **THE STORE FIRST, THE ENGINES AFTER.** Reading the allowances calls every
    // provider, and a screen that waits for that opens blank for seconds.
    if (readings === undefined) ask(false);
  }, [native, readings === undefined, ask]);

  if (seen.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute" data-bad>I could not read it: {seen.why}</p>
      </div>
    );
  }
  if (seen.state === "asking") return <div className="now"><p className="now__mute">Asking…</p></div>;

  const shown = seen.value;
  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Accounts</h2>
        <button type="button" className="now__act" onClick={() => ask(true)} disabled={reading}>
          {reading ? "Asking the engines…" : "Read the allowances"}
        </button>
      </header>

      {shown.length === 0 ? (
        <p className="now__empty">No account is declared, and nothing has spent anything.</p>
      ) : (
        <div className="account__grid">
          {shown.map((one) => (
            <section
              className="panel__block account"
              key={`${one.cli}/${one.profile ?? "—"}`}
              data-standing={one.standing}
              data-active={one.active || undefined}
            >
              <div className="account__head">
                <BrandMark slug={one.brand} label={one.label} size={20} />
                <div className="account__who">
                  <div className="account__name">{accountName(one)}</div>
                  <div className="now__why">{one.label}</div>
                </div>
                <span className="account__standing" data-standing={one.standing}>
                  {STANDING_WORD[one.standing]}
                </span>
              </div>

              {one.windows.length > 0 ? (
                <div className="account__windows">
                  {one.windows.map((window) => (
                    <Window key={window.unit} one={window} />
                  ))}
                </div>
              ) : (
                <p className="now__why">
                  {one.quota_refused ?? "The allowances are not read yet."}
                </p>
              )}

              <Did one={one} />

              {/* THE ENGINE'S OWN WORDS, and the line that cures the row. A
                  verdict with no evidence beside it is the thing this product
                  refuses to ship. */}
              {one.said !== "" && <p className="now__why account__said">{one.said}</p>}
              {one.repair !== null && <code className="now__command">{one.repair}</code>}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
