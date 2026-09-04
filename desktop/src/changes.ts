/**
 * What an agent changed in a workspace, read from git and shown as it is.
 * **THE FIELD NAMES ARE THE CONTRACT** written in `src-tauri/src/changes.rs`:
 * whoever changes one changes both.
 */
import { invoker } from "./engine";

function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no working tree to read"));
  return invoke<T>(command, args);
}

/** One file git reports as changed, with its two-letter porcelain status. */
export interface ChangedFile {
  path: string;
  status: string;
}

/**
 * The working tree of a workspace against its last commit. `diff` is git's
 * own text, not one computed here: a second diff would disagree with the one
 * a person runs in the terminal, and neither would say so.
 */
export interface Changes {
  root: string;
  files: ChangedFile[];
  diff: string;
}

export function workspaceChanges(root: string): Promise<Changes> {
  return ask<Changes>("workspace_changes", { root });
}

/** Opens a file in the editor the person already uses. */
export function openInEditor(path: string): Promise<void> {
  return ask<void>("open_in_editor", { path });
}

/** Who chose what opens a file: `SAILOR_EDITOR`, `VISUAL`, or nobody. */
export type OpenerKind = "declared" | "visual" | "system";

/** What will run on a file, and who chose it. */
export interface Opener {
  kind: OpenerKind;
  program: string;
  args: string[];
}

export function whoOpensFiles(): Promise<Opener> {
  return ask<Opener>("who_opens_files");
}

/**
 * What the button can honestly promise.
 *
 * **AN ASSOCIATION IS NOT AN EDITOR**: with nothing declared the file goes
 * wherever this machine sends that kind, which may only read it.
 */
export function openerWord(opener: Opener | null): string {
  if (!opener) return "open the file";
  if (opener.kind === "system") return "hand to the system";
  return `open in ${opener.program}`;
}

/** What is worth saying once, above the files, when nobody declared one. */
export function openerNote(opener: Opener | null): string | null {
  if (!opener || opener.kind !== "system") return null;
  return `No editor is declared: a file goes to \`${opener.program}\`, which hands it to whatever this machine opens that kind with — and that may not be able to write it. Name one in SAILOR_EDITOR.`;
}

/** The word a person reads for a porcelain status. */
export function statusWord(status: string): string {
  if (status === "??") return "new";
  if (status.includes("D")) return "deleted";
  if (status.includes("A")) return "added";
  if (status.includes("R")) return "renamed";
  if (status.includes("M")) return "changed";
  return status.trim() || "changed";
}
