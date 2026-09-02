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

/** The word a person reads for a porcelain status. */
export function statusWord(status: string): string {
  if (status === "??") return "new";
  if (status.includes("D")) return "deleted";
  if (status.includes("A")) return "added";
  if (status.includes("R")) return "renamed";
  if (status.includes("M")) return "changed";
  return status.trim() || "changed";
}
