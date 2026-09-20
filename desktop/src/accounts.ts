/**
 * The accounts Sailor can reach. **THE FIELD NAMES ARE THE CONTRACT** written
 * in `src-tauri/src/accounts.rs`, and what decides what an account *is* lives
 * further in still, in the function `sailor accounts` itself calls.
 */
import { invoker } from "./engine";

/** One window of an allowance, already named for a person. */
export interface Allowance {
  unit: string;
  /** The window's length where anything measured one, the provider's word where not. */
  word: string;
  /** **SPENT, NEVER LEFT**: a fraction from 0 to 1. */
  used_fraction: number;
  resets_at: string | null;
  lasts_seconds: number | null;
}

export interface ByModel {
  model: string;
  input: number;
  output: number;
}

export interface Worked {
  calls: number;
  sessions: number;
  by_model: ByModel[];
}

export type Standing = "ready" | "ran_out" | "shut" | "unknown";

export interface Account {
  cli: string;
  label: string;
  brand: string;
  profile: string | null;
  active: boolean;
  standing: Standing;
  /** The engine's own words, never derived from `standing`. */
  said: string;
  calls: number;
  spent_micros: number;
  tokens: number;
  last_call_at: number;
  ran_out_at: number | null;
  windows: Allowance[];
  quota_refused: string | null;
  worked: Worked | null;
  repair: string | null;
}

/**
 * Asking with `withQuota` calls the engines: seconds, and the network. Without
 * it the answer is the store and the profile homes, and costs nothing.
 */
export function accounts(hours: number, withQuota: boolean): Promise<Account[]> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no engine to ask"));
  return invoke<Account[]>("accounts", { hours, withQuota });
}

/** What to call an account to a person: the profile's name, or the engine's. */
export function accountName(one: Account): string {
  return one.profile ?? one.label;
}
