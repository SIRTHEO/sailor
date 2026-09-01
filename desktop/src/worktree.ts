/**
 * The trees this repository is checked out into. Outside the native shell
 * every call fails rather than pretending: a list of trees is not something a
 * page in a browser can be shown a plausible version of.
 */
import { invoker } from "./engine";

export interface Tree {
  name: string;
  path: string;
  branch: string | null;
  locked: boolean;
  prunable: boolean;
  /** The tree the window itself runs in, which cannot be taken down. */
  current: boolean;
}

function ask<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = invoker();
  if (!invoke) return Promise.reject(new Error("outside the desktop shell: no repository to read"));
  return invoke<T>(command, args);
}

export function listTrees(): Promise<Tree[]> {
  return ask<Tree[]>("worktree_list");
}

export function createTree(branch: string, name?: string): Promise<string> {
  return ask<string>("worktree_create", { branch, name });
}

export function removeTree(name: string): Promise<string> {
  return ask<string>("worktree_remove", { name });
}
