/**
 * The catalogue, the choice, and how much of a quota is gone. **THE FIELD
 * NAMES ARE THE CONTRACT** written in `src-tauri/src/models.rs`, and
 * `quotacontract.test.ts` compares the two so a rename cannot pass in silence.
 */
import { invoker } from "./engine";

function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no engine to ask"));
  return invoke<T>(command, args);
}

/**
 * One quota window. **`spent_fraction` is spent, not left**, and it is a
 * fraction from 0 to 1 — the provider answers 50.0 for half and the engine
 * already divided. Two lookalike units in one place get summed by mistake
 * exactly once, and nobody notices that once.
 */
export interface Window {
  engine: string;
  /** `five_hour`, `seven_day`, or a name this version does not know. */
  unit: string;
  /** What to call it to a person: its length where anything measured one, and
   *  the provider's own word where nothing did. */
  window: string;
  lasts_seconds: number | null;
  spent_fraction: number;
  /** In the provider's own shape, on purpose: nobody waits on this hour. */
  resets_at: string | null;
  observed_at: number;
}

export interface Priced {
  id: string;
  name: string;
  free: boolean;
  context_length: number | null;
  /** USD per million tokens. `null` is «no price in the catalogue», not zero. */
  price_in: number | null;
  price_out: number | null;
  modalities: string[];
}

export interface Choice {
  kind: string;
  /** What the configuration says. */
  chosen: string | null;
  /** What actually runs, once the free-only rule is applied. */
  in_force: string | null;
}

export interface Catalogue {
  models: Priced[];
  choices: Choice[];
}

/** Reading this goes to the network. */
export function catalogue(): Promise<Catalogue> {
  return ask<Catalogue>("models_catalogue");
}

/** **NOT AN ACCOUNT WITH ROOM.** Sailor never renews a token — that breaks the
 * command line's own access — so it repeats what the engine said. */
export interface Unreachable {
  /** «engine · account», as the reading labels it. */
  account: string;
  why: string;
}

/**
 * The engine a reading belongs to, out of its «engine · account» label: a match
 * on the whole label finds nothing where it used to find every window.
 */
export function engineOf(label: string): string {
  return label.split(" · ")[0];
}

/** What was read, and who did not answer: both, always. */
export interface Quota {
  windows: Window[];
  unreachable: Unreachable[];
}

/** Costs nothing and calls no model. */
export function quota(): Promise<Quota> {
  return ask<Quota>("quota");
}

export function setModel(kind: string, model_id: string): Promise<void> {
  return ask<void>("model_set", { kind, modelId: model_id });
}

/**
 * `window` already reads `session` or `week` wherever a length was measured;
 * this tidies the cases where none was. **NOT A CLOSED SET**: an unrecognised
 * key is shown as it reads, or a window the provider adds tomorrow would stop
 * appearing — which a person reads as «I have no such limit».
 */
export function windowName(window: string): string {
  return window.replace(/_/g, " ");
}

/**
 * A price per million tokens, as a person reads it. Below a cent the number
 * needs more places or every cheap model reads as «$0.00» — which is «free»,
 * and free is a different thing.
 */
export function perMillion(usd: number | null): string {
  if (usd === null) return "no price";
  if (usd === 0) return "free";
  return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`;
}
