import type { TerminalSummary } from "../terminal";

export interface TerminalGroup {
  /** The tree the terminals were opened in: the identity, never the name. */
  root: string;
  /** What the heading says: the name, or the path when two trees share it. */
  head: string;
  terminals: TerminalSummary[];
}

/**
 * **A TERMINAL IS OPENED IN A TREE, AND THE PANEL SAYS WHICH.** Grouped by the
 * root and headed by the name, in the order the engine lists them: two trees
 * called the same word are two groups, and their headings give the path.
 */
export function terminalGroups(all: TerminalSummary[]): TerminalGroup[] {
  const groups: TerminalGroup[] = [];
  for (const terminal of all) {
    const group = groups.find((one) => one.root === terminal.workspaceRoot);
    if (group) group.terminals.push(terminal);
    else groups.push({ root: terminal.workspaceRoot, head: terminal.workspaceName, terminals: [terminal] });
  }
  const shared = new Set(groups.filter((group, at) => groups.findIndex((other) => other.head === group.head) !== at).map((group) => group.head));
  return groups.map((group) => (shared.has(group.head) ? { ...group, head: group.root } : group));
}

/** The one the field holds: the chosen one while it exists, else the first
 *  still alive, else the first. A terminal that ended stays readable. */
export function heldTerminal(all: TerminalSummary[], chosen: string | null): TerminalSummary | null {
  return all.find((one) => one.id === chosen) ?? all.find((one) => one.alive) ?? all[0] ?? null;
}
