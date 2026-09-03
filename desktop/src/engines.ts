// The engines on this machine, as the shell answers about them: is it here,
// is it signed in, how much is left, and the gesture that signs it in or
// installs it. **THE FIELD NAMES ARE THE CONTRACT** of `src-tauri/src/engines.rs`.

import { invoker } from "./engine";

export type Presence = "present" | "absent" | "undetermined";
export type SignedIn = "yes" | "no" | "not known";

export interface SignIn {
  program: string;
  args: string[];
  /** The sign-in goes on inside the program: a browser opens, a code is typed. */
  interactive: boolean;
  note: string;
}

export interface Install {
  line: string;
  note: string;
}

export interface QuotaWindow {
  engine: string;
  unit: string;
  spent_fraction: number;
  resets_at: string | null;
  observed_at: number;
}

/** An engine set aside after saying its quota was spent: until when, in its words. */
export interface SetAside {
  until: number;
  said: string;
}

/** The cap the person wrote for this engine, and what the ledger sums for it. */
export interface Budget {
  cap_micros: number;
  window_secs: number;
  spent_micros: number | null;
  /** Why the sum is not there, when it is not. */
  spent_why: string | null;
}

export interface Engine {
  id: string;
  label: string;
  presence: Presence;
  reason: string;
  executable: string | null;
  version: string | null;
  signed_in: SignedIn;
  /** The engine's own words, or why nobody could ask. Shown, never parsed. */
  signed_in_said: string;
  profile_in_force: string | null;
  quota: QuotaWindow[];
  quota_why: string | null;
  sign_in: SignIn | null;
  install: Install | null;
  set_aside: SetAside | null;
  budget: Budget | null;
}

export interface Engines {
  workspace_root: string;
  engines: Engine[];
}

/** Reading this runs one detection and one sign-in question per engine present. */
export function engines(): Promise<Engines> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no engine to ask"));
  return invoke<Engines>("engines");
}
