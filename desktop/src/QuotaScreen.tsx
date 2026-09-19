/**
 * How much of a provider's quota is gone. **THE NUMBER SAYS SPENT, NEVER
 * LEFT**: on a window that never states its ceiling the rest is invented. The
 * catalogue is not here — a window is keyed by engine and a model by catalogue
 * id, and joining them would read as an availability nobody can know.
 */
import { useEffect, useState } from "react";
import { quota, windowName, type Quota, type Unreachable } from "./quota";

export type Ask<T> = { state: "asking" } | { state: "asked"; seen: T } | { state: "mute"; why: string };

/** A bar for a fraction already spent. Full is not a failure — it is a fact. */
function Spent({ fraction }: { fraction: number }) {
  const percent = Math.max(0, Math.min(1, fraction)) * 100;
  return (
    <div className="quota__bar" data-full={percent >= 100 || undefined}>
      <div className="quota__fill" style={{ width: `${percent}%` }} />
    </div>
  );
}

export function QuotaScreen({ native, now, readings }: { native: boolean; now: number; readings?: Ask<Quota> }) {
  const [ownWindows, setWindows] = useState<Ask<Quota>>({ state: "asking" });
  const windows = readings ?? ownWindows;

  useEffect(() => {
    if (!native) {
      setWindows({ state: "mute", why: "outside the desktop shell there is no engine to ask" });
      return;
    }
    if (readings === undefined) {
      quota().then(
        (seen) => setWindows({ state: "asked", seen }),
        (error) => setWindows({ state: "mute", why: String(error) }),
      );
    }
  }, [native, readings === undefined]);

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Quota already spent</h2>
      </header>

      <section className="panel__block">
        {windows.state === "mute" ? (
          // A CHANNEL THAT DOES NOT ANSWER IS NEVER A QUOTA OF ZERO, which is
          // the reassuring direction. The engine's own words say what to do.
          <p className="now__mute" data-bad>I could not read it: {windows.why}</p>
        ) : windows.state === "asking" ? (
          <p className="now__mute">Asking…</p>
        ) : windows.seen.windows.length === 0 && windows.seen.unreachable.length === 0 ? (
          <p className="now__empty">The engine reports no window at all.</p>
        ) : (
          <>
            {windows.seen.windows.length > 0 && (
            <table className="now__table">
              <thead>
                <tr><th>window</th><th>spent</th><th>the provider says it resets</th></tr>
              </thead>
              <tbody>
                {windows.seen.windows.map((one) => (
                  <tr key={`${one.engine}/${one.unit}`}>
                    <td className="now__entity">{windowName(one.unit)}
                      <div className="now__why">{one.engine}</div>
                    </td>
                    <td>
                      {(one.spent_fraction * 100).toFixed(1)}%
                      {/* A QUOTA AGES. Without the instant it was read at, this
                          cannot be told from yesterday's reading. */}
                      <div className="now__why">{secondsAgo(one.observed_at, now)}</div>
                      <Spent fraction={one.spent_fraction} />
                    </td>
                    <td>{one.resets_at ?? "not stated"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            )}
            <Unanswered by={windows.seen.unreachable} />
            {windows.seen.windows.length > 0 && (
            <p className="now__why">
              This is the whole person’s quota — every session, the terminal beside this
              one, yesterday’s job in the same window. It is not the cost of a run.
            </p>
            )}
          </>
        )}
      </section>
    </div>
  );
}

/** **SAILOR NEVER RENEWS A TOKEN** — it breaks the command line's own access —
 * so the cure is a person signing in, and the engine's words say so. */
function Unanswered({ by }: { by: Unreachable[] }) {
  if (by.length === 0) return null;
  return (
    <section className="panel__block">
      <h3 className="now__title">
        {by.length === 1 ? "One account did not answer" : `${by.length} accounts did not answer`}
      </h3>
      <p className="now__why">
        Not a quota of zero and not one with room: nothing is known about these.
        Sailor never renews a token — it would break the command line's own access —
        so the words below are the engine's, and they say what to do.
      </p>
      <table className="now__table">
        <tbody>
          {by.map((one) => (
            <tr key={one.account}>
              <td className="now__entity" data-bad>{one.account}</td>
              <td className="now__why">{one.why}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
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
