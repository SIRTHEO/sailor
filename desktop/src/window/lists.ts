/**
 * **THE COLUMN PICKS WHICH LIST, AND NOTHING ELSE.** The panel picks which
 * thing, and the field holds what is open. Five entries against the three of
 * `PLACES`: a row of words becomes a list at the fourth, a column of icons is
 * scanned — ADR-026.
 */
import { Database, Folder, KeyRound, Route, SquareTerminal } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { t } from "../i18n";

export type ListName = "workspaces" | "flows" | "terminals" | "data" | "keys";

export interface ListEntry {
  id: ListName;
  /** What a pointer resting on the icon says, and what a reader hears. */
  name: string;
  icon: LucideIcon;
}

/**
 * Data, not JSX, so the order and the names are one thing a judge can read.
 * `data` holds what used to be a place of its own: a run *is* the history, and
 * the ledger keeps them all.
 */
export const LISTS: ListEntry[] = [
  { id: "workspaces", name: t("window.rail.workspaces"), icon: Folder },
  { id: "flows", name: t("window.rail.flows"), icon: Route },
  { id: "terminals", name: t("window.rail.terminals"), icon: SquareTerminal },
  { id: "data", name: t("window.rail.data"), icon: Database },
  { id: "keys", name: t("window.rail.keys"), icon: KeyRound },
];

export function listEntry(place: ListName): ListEntry {
  const found = LISTS.find((entry) => entry.id === place);
  if (!found) throw new Error(`no list called ${place}`);
  return found;
}
