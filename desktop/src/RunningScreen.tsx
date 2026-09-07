// What Sailor lit on this machine and never saw end.
//
// **THE MACHINE FILLING UP IS NOTICED HERE AND WAS ANSWERED NOWHERE HERE**:
// finding which of forty processes was Sailor's meant reading `ps` and
// guessing. A row says what it is for, whether it is there, and who wants it.

import { useState } from "react";
import { useAsk, useClock } from "./ask";
import {
  freeTheMachine, portReading, standingOf, theDevPort, upFor, weightWords, whatWeighsHere,
  type Freed, type Standing, type TheLoad, type ThePort,
} from "./running";

const READ_EVERY_MS = 10000;

const WORD: Record<ReturnType<typeof standingOf>, string> = {
  leftover: "left over",
  wanted: "wanted",
  unclaimed: "unclaimed",
  ended: "ended",
};

/** What each word means, said where it is read rather than in a legend. */
const MEANS: Record<ReturnType<typeof standingOf>, string> = {
  leftover: "its run has ended and it is still up: this is what fills a machine",
  wanted: "a run is still open on it",
  unclaimed: "nobody wrote down who wanted it, which is not the same as nobody",
  ended: "the process this row named is gone; the row is what is left",
};

export function RunningScreen({ native }: { native: boolean }) {
  const { asked, again } = useAsk<Standing[]>(
    native,
    () => import("./running").then((it) => it.whatSailorLit()),
    READ_EVERY_MS,
    "outside the native shell: the engine reads what is running",
  );
  const port = useAsk<ThePort>(
    native,
    theDevPort,
    READ_EVERY_MS,
    "outside the native shell: the engine looks at the port",
  ).asked;
  const load = useAsk<TheLoad>(
    native,
    whatWeighsHere,
    READ_EVERY_MS,
    "outside the native shell: the engine looks at the machine",
  ).asked;
  const now = useClock();
  const [freeing, setFreeing] = useState(false);
  const [done, setDone] = useState<Freed[] | null>(null);
  const [refused, setRefused] = useState<string | null>(null);

  const rows = asked.state === "answered" ? asked.value : [];
  const leftOver = rows.filter((row) => standingOf(row) === "leftover");

  return (
    <div className="keeps">
      <div className="keeps__main">
        <h2 className="keeps__title">What Sailor is running</h2>
        <p className="keeps__lead">
          Every process Sailor lit and never saw end. A row names a process, not a number: a pid whose
          process is gone reads as ended even when something else has since been given that number.
        </p>
        {port.state === "answered" && portReading(port.value) !== null && (
          <p className="keeps__note" data-asks="">
            {portReading(port.value)}
          </p>
        )}
        {asked.state === "asking" && <p className="keeps__note">Looking…</p>}
        {asked.state === "mute" && <p className="keeps__note">{asked.why}</p>}
        {asked.state === "answered" && rows.length === 0 && (
          <p className="keeps__note">Sailor has nothing running here.</p>
        )}
        {rows.length > 0 && (
          <table className="keeps__table">
            <thead>
              <tr>
                <th>what for</th>
                <th>command</th>
                <th className="keeps__num">pid</th>
                <th className="keeps__num">up for</th>
                <th>standing</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const standing = standingOf(row);
                return (
                  <tr key={`${row.pid}-${row.started_at}`}>
                    <td>{row.purpose}</td>
                    <td className="now__when">{row.command}</td>
                    <td className="keeps__num">{row.pid}</td>
                    <td className="keeps__num">{upFor(row.started_at, now)}</td>
                    <td className="now__state" data-outcome={standing === "leftover" ? "broke" : undefined}>
                      {WORD[standing]}
                      <span className="now__why">{MEANS[standing]}</span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {/* THE GESTURE SAYS WHAT IT WILL DO BEFORE IT DOES IT, and how many.
            A button called «free» that might stop a run somebody is watching is
            a button nobody presses twice. */}
        {rows.length > 0 && (
          <p className="keeps__note">
            <button
              type="button"
              className="rail__all"
              disabled={freeing || leftOver.length === 0}
              onClick={() => {
                setFreeing(true);
                setRefused(null);
                freeTheMachine()
                  .then((freed) => {
                    setDone(freed);
                    again();
                  })
                  .catch((error: unknown) => setRefused(String(error)))
                  .finally(() => setFreeing(false));
              }}
            >
              {leftOver.length === 0
                ? "nothing here to free"
                : `stop the ${leftOver.length} a finished run left behind`}
            </button>
          </p>
        )}
        {refused !== null && <p className="keeps__note">Could not free the machine: {refused}</p>}
        {done !== null && (
          <ul className="keeps__note">
            {done.length === 0 && <li>Nothing was stopped.</li>}
            {done.map((one) => (
              <li key={one.pid}>
                {one.stopped
                  ? `stopped ${one.pid} (${one.purpose})`
                  : `${one.pid} (${one.purpose}) would not stop: ${one.why}. Its row stays written as running, which is better than calling it dead without being sure.`}
              </li>
            ))}
          </ul>
        )}
        {load.state === "answered" && (
          <div className="keeps__main">
            {/* **«NOTHING RUNNING» ON A GRINDING MACHINE IS THE LIE THIS ENDS.**
                The rows above are Sailor's own; what fills a machine is usually
                what never passed through it. */}
            <h2 className="keeps__title">What weighs on this machine</h2>
            {load.value.saw === "could_not_look" ? (
              <p className="keeps__note">
                This machine would not say what is running on it ({load.value.why}), so the rows above
                are Sailor's own and nothing else — which is not the same as nothing else being there.
              </p>
            ) : (
              <>
                <p className="keeps__lead">
                  Load {load.value.load[0].toFixed(2)} now, {load.value.load[1].toFixed(2)} over five
                  minutes, {load.value.load[2].toFixed(2)} over fifteen.
                </p>
                <table className="keeps__table">
                  <thead>
                    <tr>
                      <th className="keeps__num">size</th>
                      <th className="keeps__num">pid</th>
                      <th>command</th>
                      <th>whose</th>
                    </tr>
                  </thead>
                  <tbody>
                    {load.value.heaviest.map((one) => (
                      <tr key={one.pid}>
                        <td className="keeps__num">{weightWords(one.kilobytes)}</td>
                        <td className="keeps__num">{one.pid}</td>
                        <td className="now__when">{one.command}</td>
                        <td>{one.sailor_lit ? "sailor lit it" : "sailor cannot free it"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
