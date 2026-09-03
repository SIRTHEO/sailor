/**
 * The places of the window. **THREE GROUNDS, NOT ONE LIST**: what belongs to
 * the tree you stand in hangs under it, what belongs to no tree sits outside,
 * and what belongs to THIS MACHINE is the same wherever you stand. Folded into
 * one noun called «Sailor», those seven cost two clicks and were named nowhere.
 */
import type { SailorTab } from "./sailortabs";
import { SAILOR_TABS } from "./sailortabs";

export type Section = "board" | "terminals" | "ledger" | "memory" | "sailor";

export interface Place {
  id: Section;
  name: string;
  /** The mark beside the name. It carries no state: it makes a row findable. */
  glyph: string;
  /** The question the section answers, shown where the section opens. */
  asks: string;
  group: "work" | "what happened" | "itself";
}

export const PLACES: Place[] = [
  { id: "board", name: "Board", glyph: "◈", asks: "what am I doing", group: "work" },
  { id: "terminals", name: "Terminals", glyph: "▮", asks: "what is running", group: "work" },
  {
    id: "ledger",
    name: "Ledger",
    glyph: "▤",
    asks: "the tables, as they are",
    group: "what happened",
  },
  {
    id: "memory",
    name: "Runs",
    glyph: "◷",
    asks: "what happened, and what it cost",
    group: "what happened",
  },
  { id: "sailor", name: "Sailor", glyph: "⚓", asks: "what it knows, what it can do", group: "itself" },
];


/* THE GROUND THAT DOES NOT CHANGE WHEN YOU CHANGE PROJECT. A row is a place
   plus, where the place has tabs, which tab — so one click lands on the thing
   itself and not on a column that asks again. */

/** What the column writes over that ground, and what the bar says you are in. */
export const MACHINE_GROUND = "this mac";

/** A place of that ground, and the tab inside it when the place has tabs. */
export interface MachineRow {
  id: string;
  name: string;
  glyph: string;
  asks: string;
  section: Section;
  tab?: SailorTab;
}

/**
 * The order is what a person needs first: what runs the work, under whose
 * account, with which model — then what this machine holds. `sailor_cmd` knows
 * the tabs; `machineHolds` refuses one that lost its row, so a tab added later
 * cannot go back into hiding.
 */
export const MACHINE: MachineRow[] = [
  { id: "engines", name: "Engines", glyph: "\u2699", asks: "which command lines are here, signed in, and how full", section: "sailor", tab: "engines" },
  { id: "profiles", name: "Profiles", glyph: "\u25d1", asks: "which account each command line runs under", section: "sailor", tab: "profiles" },
  { id: "models", name: "Models", glyph: "\u25cd", asks: "the catalogue, and which is in use", section: "sailor", tab: "models" },
  { id: "equipment", name: "Equipment", glyph: "\u2692", asks: "tools, skills and rules on this machine", section: "sailor", tab: "equipment" },
  { id: "ledger", name: "Ledger", glyph: "\u25a4", asks: "the tables, as they are", section: "ledger" },
  { id: "commands", name: "Commands", glyph: "\u2318", asks: "every verb sailor answers to", section: "sailor", tab: "commands" },
  { id: "keeps", name: "Stores", glyph: "\u25a3", asks: "every store, its path and its size", section: "sailor", tab: "keeps" },
  { id: "cando", name: "What it can do", glyph: "\u2726", asks: "the actions a flow may use", section: "sailor", tab: "cando" },
];

/** The tabs the machine ground carries, for the check that keeps the two glued. */
export function machineHolds(): SailorTab[] {
  return MACHINE.flatMap((row) => (row.tab === undefined ? [] : [row.tab]));
}

/** Every tab a shipped screen declares. Read, never copied. */
export function tabsThatExist(): SailorTab[] {
  return SAILOR_TABS.map((tab) => tab.id);
}

/**
 * The places the strip above the work carries: the ones that belong to no
 * ground below it. The board hangs under the tree it draws, and a place the
 * machine's ground already holds is not offered twice.
 */
export function inTheStrip(): Place[] {
  return PLACES.filter(
    (place) => place.id !== "board" && !MACHINE.some((row) => row.section === place.id),
  );
}
