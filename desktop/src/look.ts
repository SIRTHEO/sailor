/**
 * **THE LOOK OF THE WINDOW, AND WHO DECIDES IT.** The machine used to decide
 * alone, through `prefers-color-scheme`; there are three answers now, «the
 * machine's» among them and not as the absence of one. `data-theme` on the
 * root is what the sheet listens to, and the machine's answer is no attribute.
 */

export type Look = "the machine's" | "night" | "day";

/** The three, in the order they are offered: the default first. */
export const LOOKS: Look[] = ["the machine's", "night", "day"];

/** What the sheet is told, per look. `null` means: say nothing, let it ask. */
const STAMP: Record<Look, string | null> = {
  "the machine's": null,
  night: "dark",
  day: "light",
};

const KEY = "sailor.look";

/** A store that is not there is not a fault: the look is then the machine's. */
function store(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The look this machine was left on. Anything unreadable is the machine's. */
export function savedLook(): Look {
  const kept = store()?.getItem(KEY);
  return LOOKS.find((look) => look === kept) ?? "the machine's";
}

/** Puts the look on, and remembers it. */
export function wear(look: Look, root: HTMLElement = document.documentElement): void {
  const stamp = STAMP[look];
  if (stamp === null) root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", stamp);
  try {
    store()?.setItem(KEY, look);
  } catch {
    // A refused store loses the choice at the next start, not this one.
  }
}

/** Called once, before anything is drawn: the window opens as it was left. */
export function wearSavedLook(root: HTMLElement = document.documentElement): Look {
  const look = savedLook();
  const stamp = STAMP[look];
  if (stamp === null) root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", stamp);
  return look;
}

/** What the machine says right now, for the row that offers to follow it. */
export function whatTheMachineSays(): "night" | "day" {
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "day" : "night";
}
