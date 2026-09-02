/**
 * The projects Sailor has been opened in. **THE FIELD NAMES ARE THE CONTRACT**
 * written in `src-tauri/src/workspaces.rs`: whoever changes one changes both,
 * and `workspaces.test.ts` compares the two so a rename cannot pass in silence.
 */
import { invoker } from "./engine";

/**
 * Outside the native shell every call fails rather than pretending. A list of
 * projects is read from the home on disk: a browser cannot be shown a
 * plausible version of it, and one would teach something untrue.
 */
function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no home to read"));
  return invoke<T>(command, args);
}

/** Whether the marker is still where the project was left. */
export type Standing = "declared" | "gone";

export interface Project {
  root: string;
  name: string;
  first_seen: number;
  last_seen: number;
  standing: Standing;
  /** The project the window is standing in, if it is one of these. */
  current: boolean;
}

/** What a project declares about itself in its own `sailor.json`. */
export interface Declaration {
  name: string;
  rules: string[];
  checks: Record<string, string>;
  equipment: string | null;
}

/**
 * The projects, most recently opened first.
 *
 * An empty list is an answer, not a failure: nobody has declared a project yet,
 * and the screen that shows it says how to declare one.
 */
export function projects(): Promise<Project[]> {
  return ask<Project[]>("workspaces");
}

/** What one project declares. Read when someone looks at it, not per row. */
export function declarationOf(root: string): Promise<Declaration> {
  return ask<Declaration>("workspace_declaration", { root });
}

/**
 * How long ago, in the words a person uses. Not a date: a list of projects is
 * scanned for «which one was I in», and «3 days ago» answers that where
 * «2026-08-30» has to be worked out.
 */
export function since(seconds: number, now: number): string {
  const gap = Math.max(0, now - seconds);
  if (gap < 90) return "just now";
  const minutes = Math.round(gap / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days} d ago`;
  return `${Math.round(days / 30)} mo ago`;
}
