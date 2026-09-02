/**
 * What the ledger holds. **THE FIELD NAMES ARE THE CONTRACT** written in
 * `src-tauri/src/ledger.rs`; `heldcontract.test.ts` compares the two.
 */
import { invoker } from "./engine";

export interface Leftover {
  process_id: string;
  pid: number;
  command: string;
  working_directory: string;
  port: number | null;
  /** Asked now, not assumed: a record left open is not a process still running. */
  alive: boolean;
}

export interface OpenRun {
  run_id: string;
  entity: string;
  open_steps: number;
  oldest_started_at: number;
}

export interface Waiting {
  run_id: string;
  entity: string;
  waiting_since: number;
}

export interface FailureClass {
  /** `null` is a failure the engine could not classify — not a class named so. */
  class: string | null;
  failures: number;
  runs_affected: number;
}

export interface Kept {
  collection: string;
  key: string;
}

export interface Held {
  directory: string;
  /** «Not created yet» and «empty» are different facts, and this says which. */
  exists: boolean;
  runs: number;
  unfinished: OpenRun[];
  waiting: Waiting[];
  leftovers: Leftover[];
  failures: FailureClass[];
  kept: Kept[];
  inventory_present: number;
  inventory_gone: number;
}

export function held(): Promise<Held> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no ledger to read"));
  return invoke<Held>("ledger_held");
}
