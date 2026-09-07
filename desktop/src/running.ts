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

/**
 * What is on the window's own development port. **THE CASE THAT FILLED THIS
 * MACHINE**: a server started by hand holds it, sits in no row, and no sweep
 * will ever find it.
 */
export type ThePort =
  | { on: "free" }
  | { on: "ours"; pid: number; purpose: string }
  | { on: "somebody" }
  /** **UNKNOWN IS NOT FREE**: a machine that would not let us look said so. */
  | { on: "could_not_look"; why: string };

export async function theDevPort(): Promise<ThePort> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: nothing to look at");
  return invoke<ThePort>("the_dev_port");
}

/** What the screen says about the port, or nothing when there is nothing to say. */
export function portReading(port: ThePort): string | null {
  switch (port.on) {
    // A port Sailor lit is already a row above; saying it twice teaches the
    // reader that this line carries no news.
    case "free":
    case "ours":
      return null;
    case "somebody":
      return "something sailor never lit is holding the window's development port: what did not pass through sailor is what sailor cannot free, so this one is yours to stop";
    case "could_not_look":
      return `this machine would not let us look at the window's development port (${port.why}), so whether anything holds it is unknown — and unknown is not free`;
  }
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
 * What a row is, in one word. **«LEFTOVER» IS THE POINT OF THE SCREEN**: alive
 * and wanted looks like alive and forgotten if the only question is whether it
 * breathes, and the forgotten ones fill a machine.
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
