/**
 * The register of what has broken. **THE FIELD NAMES ARE THE CONTRACT** written
 * in `src-tauri/src/faults.rs`; `registercontract.test.ts` compares the two.
 */
import { invoker } from "./engine";

function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no register to read"));
  return invoke<T>(command, args);
}

/**
 * **FOUR ANSWERS, AND THE FOURTH IS THE POINT.** `unrecognised` is prose the
 * register was never taught: refusing it keeps a fault from being subtracted
 * from the open count by a wording nobody meant to change anything with.
 */
export type Standing = "open" | "partly closed" | "closed" | "unrecognised";

export interface Entry {
  number: number;
  happened_on: string;
  what_happened: string;
  how_it_showed: string;
  /** The column that separates this from a diary. */
  what_would_prevent: string;
  status: string;
  standing: Standing;
}

export interface Register {
  entries: Entry[];
  path: string;
  still_open: number;
}

export function register(): Promise<Register> {
  return ask<Register>("faults");
}

export function setStatus(number: number, status: string): Promise<void> {
  return ask<void>("fault_status", { number, status });
}

/** The prose the register writes, so the window offers exactly those words. */
export const STATUS_WORDS: Record<Exclude<Standing, "unrecognised">, string> = {
  open: "**aperto**",
  "partly closed": "**chiuso in parte**",
  closed: "**chiuso**",
};
