// What Sailor keeps: every store with its real path, count and size; the
// version in service; the project root. A missing store is said, not zeroed.

import { useAsk } from "./ask";
import { sizeWords, whatSailorKeeps } from "./keeps";

const KEEPS_EVERY_MS = 30000;

function when(seconds: number | null): string {
  if (seconds === null) return "unknown";
  return new Date(seconds * 1000).toLocaleString("en-GB", { hour12: false });
}

export function KeepsScreen({ native }: { native: boolean }) {
  const { asked, again } = useAsk(native, whatSailorKeeps, KEEPS_EVERY_MS, "outside the native shell: nothing to look at");

  return (
    <div className="keeps">
      <div className="keeps__main">
        <h2 className="keeps__title">What Sailor keeps</h2>
        <p className="keeps__lead">
          Everything Sailor keeps, where it actually lives, and how much room it takes. A thing whose place you do
          not know is a thing you do not control.
        </p>
        {asked.state === "asking" && <p className="keeps__note">Looking…</p>}
        {asked.state === "mute" && <p className="keeps__note">{asked.why}</p>}
        {asked.state === "answered" && (
          <>
            <table className="keeps__table">
              <thead>
                <tr>
                  <th>what</th>
                  <th>where</th>
                  <th className="keeps__num">how many</th>
                  <th className="keeps__num">size</th>
                </tr>
              </thead>
              <tbody>
                {asked.value.stores.map((store) => (
                  <tr key={store.what} data-missing={!store.exists || undefined}>
                    <td>{store.what}</td>
                    <td className="keeps__path">{store.where}</td>
                    {store.exists ? (
                      <>
                        <td className="keeps__num">{store.how_many ?? "–"}</td>
                        <td className="keeps__num">{store.bytes === null ? "–" : sizeWords(store.bytes)}</td>
                      </>
                    ) : (
                      <td className="keeps__missing" colSpan={2}>
                        not created yet — nothing written here
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="keeps__note">
              <strong>An empty row is the point.</strong> A store that is missing, shown as a plausible count, would
              not be hiding: it would be telling you something false.
            </p>
            <button type="button" className="keeps__again" onClick={again}>
              look again
            </button>
          </>
        )}
      </div>

      {asked.state === "answered" && (
        <aside className="keeps__side">
          <div className="places__heading">the home</div>
          <div className="keeps__path">{asked.value.home}</div>
          <div className="keeps__small">
            {sizeWords(asked.value.home_bytes)} · {asked.value.home_files} files
          </div>

          <div className="places__heading keeps__gap">version in service</div>
          <dl className="keeps__facts">
            <div>
              <dt>window</dt>
              <dd>{asked.value.in_service.window_version}</dd>
            </div>
            <div>
              <dt>binary</dt>
              <dd className="keeps__path">{asked.value.in_service.binary ?? "no sailor on the search path"}</dd>
            </div>
            <div>
              <dt>built</dt>
              <dd>{when(asked.value.in_service.built_at)}</dd>
            </div>
            <div>
              <dt>from</dt>
              <dd>
                {asked.value.in_service.commit === null
                  ? "no release stamp: not put in service by sailor release"
                  : `sources ${asked.value.in_service.commit.slice(0, 8)}`}
              </dd>
            </div>
          </dl>
          <p className="keeps__small">
            To put HEAD into service: <code>sailor release sailor</code>, then copy the binary where the hooks call it.
          </p>

          <div className="places__heading keeps__gap">project root</div>
          <div className="keeps__path">
            {asked.value.project_root ?? "none: no sailor.json above the working directory"}
          </div>
        </aside>
      )}
    </div>
  );
}
