/**
 * **A REBUILD IS NOT A REASON TO LOSE YOUR PLACE.** This window is replaced
 * whenever the engine under it is built again, and came back on the board at
 * the first flow of the list: the terminals already outlive that, and this is
 * the rest of it. Kept in the window's own storage, like the look.
 */

const KEY = "sailor.where";

/** Where somebody was, as far as it can be written down. */
export interface Where {
  place?: string;
  sailorTab?: string;
  memoryTab?: string;
  /** The flow the board had open, or `null` for none. */
  focus?: string | null;
  /** The bench: which terminal judges which handed step, kept for the same
   * reason as the rest — the terminal survives the swap, the strip must too. */
  bench?: { terminalId: string; runId: string; stepId: string; mandate: string } | null;
}

function store(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** What was written down last time, or nothing at all. */
export function whereYouWere(): Where {
  try {
    const kept = store()?.getItem(KEY);
    if (kept === null || kept === undefined) return {};
    const read: unknown = JSON.parse(kept);
    // A HALF-WRITTEN NOTE IS NOT A PLACE. Anything that is not an object is
    // dropped whole rather than read field by field into a broken window.
    if (typeof read !== "object" || read === null || Array.isArray(read)) return {};
    return read as Where;
  } catch {
    return {};
  }
}

/** Writes down where you are now, merged over what was already there. */
export function rememberWhere(some: Where): void {
  try {
    const merged = { ...whereYouWere(), ...some };
    store()?.setItem(KEY, JSON.stringify(merged));
  } catch {
    // A refused store costs the walk back once, and nothing else.
  }
}

/**
 * The one of `offered` that was last open, or the first. **Never a name the
 * window no longer has**: a tab that was renamed or dropped would leave the
 * screen on nothing, which is worse than opening where everybody starts.
 */
export function amongThese<T extends string>(kept: string | undefined, offered: readonly T[], fallback: T): T {
  return offered.find((one) => one === kept) ?? fallback;
}
