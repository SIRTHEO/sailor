/**
 * What this machine offers, as one sweep sees it. **THE FIELD NAMES ARE THE
 * CONTRACT** written in `src-tauri/src/tools.rs`; `machinecontract.test.ts`
 * compares the two sides.
 */
import { invoker } from "./engine";

/**
 * **THREE ANSWERS, NOT TWO.** `absent` is «it is not here», `undetermined` is
 * «I could not look» — one is cured by installing, the other by finding out
 * why the check would not run. Merged, they make people install a second copy
 * of what they already have.
 */
export type Presence = "present" | "absent" | "undetermined";

export interface Tool {
  id: string;
  name: string;
  kind: string;
  path: string | null;
  version: string | null;
  /** «Can I use it», which for a tool nobody looked at is no. */
  available: boolean;
  presence: Presence;
  /** Why it is so. Without this a list cannot be corrected. */
  reason: string;
  descriptor: string;
}

/** A line of the list of what to look for that would not read. */
export interface BadLine {
  source: string;
  about: string;
  reason: string;
}

export interface Sweep {
  tools: Tool[];
  looked_in: string[];
  problems: BadLine[];
}

export function sweep(): Promise<Sweep> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no machine to sweep"));
  return invoke<Sweep>("tools_sweep");
}

/** What each state is called on screen. Nothing is called nothing. */
export const PRESENCE_WORD: Record<Presence, string> = {
  present: "here",
  absent: "not here",
  undetermined: "could not look",
};
