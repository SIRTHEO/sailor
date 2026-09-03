/**
 * How the trees of a workspace are grouped and named: a workspace has several
 * — one checkout per branch — so the name groups and the trees sit under it.
 * The switcher went: the column already shows every project and every tree.
 */
import type { Project } from "./workspaces";

export interface Grouped {
  name: string;
  trees: Project[];
}

/** Groups the trees under the name their marker declares. */
export function grouped(seen: Project[]): Grouped[] {
  const byName = new Map<string, Project[]>();
  for (const one of seen) {
    const trees = byName.get(one.name);
    if (trees) trees.push(one);
    else byName.set(one.name, [one]);
  }
  return Array.from(byName, ([name, trees]) => ({
    name,
    trees: trees.slice().sort((left, right) => right.last_seen - left.last_seen),
  })).sort((left, right) => right.trees[0].last_seen - left.trees[0].last_seen);
}

/** The last segment of a path: what a person calls that tree. */
export function treeName(root: string): string {
  const parts = root.split("/").filter((part) => part !== "");
  return parts[parts.length - 1] ?? root;
}
