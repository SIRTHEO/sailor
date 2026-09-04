/**
 * The profiles of the command lines Sailor knows. **THE FIELD NAMES ARE THE
 * CONTRACT** written in `src-tauri/src/profiles.rs`, and `profilecontract.test.ts`
 * compares the two so a rename cannot pass in silence.
 */
import { invoker } from "./engine";

function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no engine to ask"));
  return invoke<T>(command, args);
}

/**
 * Whether a command line does profiles on its own. `unverified` is **not** a
 * no: nobody checked, and the note says why — the two lead to different
 * gestures, and collapsing them would invent a fact.
 */
export type Native = "supported" | "not supported" | "unverified";

/**
 * How a command line's home moves. `none` means it does not: two profiles of
 * that command line start it in the same place, however many you make.
 */
export type Mechanism = "variable" | "symlink" | "none";

export interface CommandLine {
  id: string;
  display_name: string;
  executable: string;
  native_profiles: Native;
  native_profiles_note: string;
  home_mechanism: Mechanism;
  /** The variable's name, or the swapped path. Empty when there is neither. */
  home_detail: string;
  home_note: string;
  /** The home it already keeps here, whole. Empty where nobody found one. */
  home_already_here: string;
}

/**
 * What the engine says about one profile's home. `not known` is nobody looked,
 * and is neither a yes nor a no; `home does not move` says the profile is real
 * but changes nothing.
 */
export type Access = "yes" | "no" | "not known" | "home does not move";

export interface Row {
  cli_id: string;
  name: string;
  home_dir: string;
  active: boolean;
  access: Access;
  /** The engine's own words, in the reader's language. Shown, never parsed. */
  said: string;
}

export function commandLines(): Promise<CommandLine[]> {
  return ask<CommandLine[]>("profile_command_lines");
}

/** Reading this runs one command per profile — see the note on the Rust side. */
export function rows(): Promise<Row[]> {
  return ask<Row[]>("profiles");
}

export function switchTo(cli_id: string, name: string): Promise<void> {
  return ask<void>("profile_switch", { cliId: cli_id, name });
}

export function adopt(cli_id: string, name: string): Promise<void> {
  return ask<void>("profile_adopt", { cliId: cli_id, name });
}

/**
 * The home worth adopting here, or null. **A HOME ALREADY TAKEN IS NOT
 * OFFERED TWICE**: two profiles on one directory are one account under two
 * names, and switching between them changes nothing anybody can see.
 */
export function toAdopt(cli: CommandLine, rows: Row[]): string | null {
  const at = cli.home_already_here;
  if (at === "") return null;
  const taken = rows.some((row) => row.cli_id === cli.id && row.home_dir === at);
  return taken ? null : at;
}

export function create(cli_id: string, name: string): Promise<void> {
  return ask<void>("profile_create", { cliId: cli_id, name });
}
