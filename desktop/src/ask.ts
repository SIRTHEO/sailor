// Asking the engine something, and saying honestly how it went.
//
// **THREE OUTCOMES, NOT TWO.** «I have the answer» and «I do not» are not enough:
// between them sits «I could not ask», and that is the one to spell out. An empty
// screen because nothing ran and one because the engine is not answering look far
// too alike, and the second is the one where you keep working believing nothing is
// happening.
//
// WHY ONE HOOK AND NOT THREE COPIES. «Now», the history and the inventory all do
// the same thing: ask the shell, repeat now and then, and must be able to say why
// they have no answer. Written three times, the third omits the silence branch.

import { useCallback, useEffect, useState } from "react";

/** How the question went, from the point of view of whoever is watching. */
export type Asked<T> =
  | { state: "asking" }
  | { state: "answered"; value: T }
  | { state: "mute"; why: string };

/**
 * Asks the engine, and repeats while the screen is open.
 *
 * `every` in milliseconds, or `null` to ask just once: a census of the disk is
 * not to be redone every four seconds, a live run is.
 */
export function useAsk<T>(
  native: boolean,
  question: () => Promise<T>,
  every: number | null,
  outside: string,
): { asked: Asked<T>; again: () => void } {
  const [asked, setAsked] = useState<Asked<T>>(() =>
    native ? { state: "asking" } : { state: "mute", why: outside },
  );

  const again = useCallback(() => {
    if (!native) return;
    question()
      .then((value) => setAsked({ state: "answered", value }))
      .catch((error: unknown) => setAsked({ state: "mute", why: String(error) }));
    // `question` is out of the dependencies on purpose: callers write it inline,
    // so it changes on every render, and putting it here would redo the question
    // on every draw instead of on every beat.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [native]);

  useEffect(() => {
    if (!native) return;
    again();
    if (every === null) return;
    const tick = window.setInterval(again, every);
    return () => window.clearInterval(tick);
  }, [native, every, again]);

  return { asked, again };
}

/**
 * A clock that beats, to age the durations on screen.
 *
 * **WITHOUT IT, «still for 2 min» STAYS WRITTEN FOR AN HOUR.** The line would
 * look alive and be frozen: the same fault by which a view showed «running for
 * 00:30» on a run long since finished.
 */
export function useClock(): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(tick);
  }, []);
  return now;
}
