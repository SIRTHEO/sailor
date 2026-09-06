/**
 * How much of a provider's quota is gone. **THE NUMBER SAYS SPENT, NEVER
 * LEFT**: on a window that never states its ceiling the rest is invented. The
 * catalogue is not here — a window is keyed by engine and a model by catalogue
 * id, and joining them would read as an availability nobody can know.
 */
import { useEffect, useState } from "react";
import { quota, windowName, type Window as QuotaWindow } from "./quota";

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

export function QuotaScreen({ native, now, readings }: { native: boolean; now: number; readings?: Ask<QuotaWindow[]> }) {
  const [ownWindows, setWindows] = useState<Ask<QuotaWindow[]>>({ state: "asking" });
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
        ) : windows.seen.length === 0 ? (
          <p className="now__empty">The engine reports no window at all.</p>
        ) : (
          <>
            <table className="now__table">
              <thead>
                <tr><th>window</th><th>spent</th><th>the provider says it resets</th></tr>
              </thead>
              <tbody>
                {windows.seen.map((one) => (
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
            <p className="now__why">
              This is the whole person’s quota — every session, the terminal beside this
              one, yesterday’s job in the same window. It is not the cost of a run.
            </p>
          </>
        )}
      </section>
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
