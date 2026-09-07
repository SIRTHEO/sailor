// What Sailor lit on this machine and never saw end, and the gesture that puts
// it out. The same reading the command line gives: a second one written here is
// how the window and `sailor machine` would come to disagree about what is
// running, and only one of them can be right.

import { invoker } from "./engine";

export interface Standing {
  pid: number;
  purpose: string;
  command: string;
  /**
   * Whether the process the row named is still there — **not** whether
   * something now holds the number it was given. Pid numbers come round.
   */
  still_there: boolean;
  run_id: string | null;
  /** True when that run has ended, or when no run ever claimed it. */
  run_is_over: boolean;
  started_at: number;
}

export interface Freed {
  pid: number;
  purpose: string;
  stopped: boolean;
  /** Why it is still there, when it is. Empty when it went. */
  why: string;
}

export async function whatSailorLit(): Promise<Standing[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: nothing to look at");
  return invoke<Standing[]>("what_sailor_lit");
}

export async function freeTheMachine(): Promise<Freed[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: nothing to stop");
  return invoke<Freed[]>("free_the_machine");
}

/**
 * What a row is, in one word.
 *
 * **«LEFTOVER» IS THE WHOLE POINT OF THE SCREEN.** Alive and wanted looks
 * exactly like alive and forgotten if the only question asked is whether it
 * breathes — and it is the forgotten ones that fill a machine.
 */
export type Standingness = "leftover" | "wanted" | "unclaimed" | "ended";

export function standingOf(row: Standing): Standingness {
  if (!row.still_there) return "ended";
  if (!row.run_is_over) return "wanted";
  // A run that ended left it behind; no run at all means nobody wrote down who
  // wanted it, and that is a different fact — it is shown and left alone.
  return row.run_id === null ? "unclaimed" : "leftover";
}

/** How long it has been up, from the second it was recorded. */
export function upFor(startedAt: number, now: number): string {
  const seconds = Math.max(0, now - startedAt);
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} h ${minutes % 60} min`;
  return `${Math.floor(hours / 24)} d ${hours % 24} h`;
}
