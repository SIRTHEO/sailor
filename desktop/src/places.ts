/**
 * The places of the window. **THREE GROUNDS, NOT ONE LIST**: what belongs to
 * the tree you stand in hangs under it, what belongs to no tree sits outside,
 * and what belongs to THIS MACHINE is the same wherever you stand. Folded into
 * one noun called «Sailor», those seven cost two clicks and were named nowhere.
 */
import type { MemoryTab } from "./memorytabs";
import type { SailorTab } from "./sailortabs";
import { SAILOR_TABS } from "./sailortabs";

export type Section = "board" | "changes" | "sketch" | "terminals" | "memory" | "sailor";

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
  {
    id: "changes",
    name: "Changes",
    glyph: "⇄",
    asks: "what is not saved yet, in this tree",
    group: "work",
  },
  {
    id: "sketch",
    name: "Whiteboard",
    glyph: "✎",
    asks: "draw the flow you want, in blocks and words",
    group: "work",
  },
  { id: "terminals", name: "Terminals", glyph: "▮", asks: "what is running", group: "work" },
  {
    id: "memory",
    name: "Runs",
    glyph: "◷",
    asks: "what happened, what it cost, and the tables under it",
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
  /** Set when the row lands on a view of the history rather than on a screen. */
  memoryTab?: MemoryTab;
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
  { id: "ledger", name: "Ledger", glyph: "\u25a4", asks: "the tables, as they are", section: "memory", memoryTab: "ledger" },
  { id: "commands", name: "Commands", glyph: "\u2318", asks: "every verb sailor answers to", section: "sailor", tab: "commands" },
  { id: "keeps", name: "Stores", glyph: "\u25a3", asks: "every store, its path and its size", section: "sailor", tab: "keeps" },
  { id: "cando", name: "What it can do", glyph: "\u2726", asks: "the actions a flow may use", section: "sailor", tab: "cando" },
  { id: "look", name: "Appearance", glyph: "\u263e", asks: "night, day, or whatever this machine says", section: "sailor", tab: "look" },
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
 * Whether the machine's ground stands in for a place instead of reaching into
 * it. **A ROW THAT OPENS ONE VIEW OF A PLACE IS NOT THE PLACE**: the ledger is
 * one view of the history among four, and reading the section as taken would
 * cost the other three their only name.
 */
export function namedByTheMachine(id: Section): boolean {
  const rows = MACHINE.filter((row) => row.section === id);
  return rows.length > 0 && rows.every((row) => row.memoryTab === undefined);
}

/**
 * The places the strip above the work carries: the ones that belong to no
 * ground below it. The board hangs under the tree it draws, and a place the
 * machine's ground already holds is not offered twice.
 */
export function inTheStrip(): Place[] {
  return PLACES.filter((place) => !UNDER_A_TREE.includes(place.id) && !namedByTheMachine(place.id));
}

/**
 * The places that belong to the tree you stand in, and hang under it. Not a
 * strip row: what they answer changes with the tree, and a row that answers
 * about a tree you cannot see from it is a row that lies.
 */
export const UNDER_A_TREE: Section[] = ["board", "changes", "sketch"];

/**
 * Every place ⌘K offers under its own name. **A PLACE OUTSIDE THE STRIP IS
 * STILL A PLACE**: built from the strip, the list left out the three that hang
 * under a tree. The two the machine's ground names better are left to it, so
 * «Ledger» is offered once and not twice.
 */
export function onItsOwnName(): Place[] {
  return PLACES.filter((place) => !namedByTheMachine(place.id));
}
