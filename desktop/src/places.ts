/**
 * The places are **QUESTIONS, NOT SCREENS**: what I am doing, why it was done
 * that way, what is set up here. Named as screens, a person had to know the
 * product to find anything. Both grounds now carry a name.
 */
import { t } from "./i18n";
import type { MemoryTab } from "./memorytabs";
import type { SailorTab } from "./sailortabs";
import { SAILOR_TABS } from "./sailortabs";

export type Section =
  | "waiting"
  | "board"
  | "changes"
  | "sketch"
  | "terminals"
  | "flowmap"
  | "memory"
  | "sailor";

export interface Place {
  id: Section;
  name: string;
  /** The mark beside the name. It carries no state: it makes a row findable. */
  glyph: string;
  /** The question the section answers, shown where the section opens. */
  asks: string;
  group: "work" | "what happened" | "itself";
}

/** **ONE NAME, NOT TWO.** The bar wrote «this mac» over a row the list called
 * «The machine»: a place and the ground it stands on are one thing. */
export const MACHINE_GROUND = t("window.ground.machine");

/** **THE WINDOW IS THE ARRANGEMENT OF THE TERMINALS**, and this is the name
 * the bar, the palette and the list all give it. */
export const TERMINALS_GROUND = t("window.ground.terminals");

/**
 * **WHAT THE WINDOW IS FOR**: to understand why a thing was done as it was, on
 * which engine, at what cost. `board`, `changes` and `sketch` are absent on
 * purpose — they answer about one tree, and hang under it.
 */
export const PLACES: Place[] = [
  {
    id: "waiting",
    name: "Waiting for you",
    glyph: "\u25e8",
    asks: "what wants a decision from you, and what happened while you were away",
    group: "work",
  },
  {
    id: "terminals",
    name: TERMINALS_GROUND,
    glyph: "\u25ae",
    asks: "the command lines open now, and what they are costing",
    group: "work",
  },
  {
    id: "flowmap",
    name: "Which calls which",
    glyph: "\u2442",
    asks: "which flow calls which, and which call nothing at all",
    group: "work",
  },
  {
    id: "memory",
    name: "Why",
    glyph: "\u25f7",
    asks: "why each thing was done as it was, what it cost, and the store under it",
    group: "what happened",
  },
  {
    id: "sailor",
    name: MACHINE_GROUND,
    glyph: "\u2693",
    asks: "what is set up here, the same wherever you stand",
    group: "itself",
  },
];

/**
 * Every section the window can stand in, offered or not. **A PLACE LIST IS FOR
 * CHOOSING; THIS IS FOR COMING BACK**: what the machine's screens live in is
 * named by no row of `PLACES`, and reading the two as one list would send
 * whoever left the window on Profiles back to the board.
 */
export const SECTIONS: Section[] = [
  "waiting",
  "board",
  "changes",
  "sketch",
  "terminals",
  "flowmap",
  "memory",
  "sailor",
];


/* THE GROUND THAT DOES NOT CHANGE WHEN YOU CHANGE PROJECT. A row is a place
   plus, where the place has tabs, which tab — so one click lands on the thing
   itself and not on a column that asks again. */



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
  { id: "running", name: "What it is running", glyph: "\u25c9", asks: "what sailor lit on this machine and never saw end", section: "sailor", tab: "running" },
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
 * Whether a row of the machine reaches into a place that carries its own name
 * elsewhere. **A ROW THAT OPENS ONE VIEW OF A PLACE IS NOT THE PLACE**: the
 * ledger is one view of the history among four, and reading the section as
 * taken would cost the other three their only name.
 */
export function namedByTheMachine(id: Section): boolean {
  const rows = MACHINE.filter((row) => row.section === id);
  return rows.length > 0 && rows.every((row) => row.memoryTab === undefined);
}


/**
 * The places that belong to the tree you stand in, and hang under it. Not a
 * strip row: what they answer changes with the tree, and a row that answers
 * about a tree you cannot see from it is a row that lies.
 */
export const UNDER_A_TREE: Section[] = ["board", "changes", "sketch"];

/** **A PLACE OUTSIDE THE LIST IS STILL A PLACE**: built from the fixed list
 * alone, the palette left out the three used while working in a tree. */
export function onItsOwnName(): Place[] {
  return [...PLACES, ...UNDER_THE_TREE];
}

/** Out of `PLACES` because their answer changes with the tree: a fixed row
 * would lie about which one it means. */
export const UNDER_THE_TREE: Place[] = [
  { id: "board", name: "Board", glyph: "\u25c8", asks: "the flow, as a graph", group: "work" },
  {
    id: "changes",
    name: "Changes",
    glyph: "\u21c4",
    asks: "what is not saved yet, in this tree",
    group: "work",
  },
  {
    id: "sketch",
    name: "Whiteboard",
    glyph: "\u270e",
    asks: "draw the flow you want, in blocks and words",
    group: "work",
  },
];

/** **ONE LOOKUP, NOT TWO**: the fixed list alone left Board printing its own
 * id in the bar, and left the capture calling the board a place the product
 * does not have. */
export function placeNamed(id: Section): Place | null {
  return onItsOwnName().find((place) => place.id === id) ?? null;
}

export function nameOfPlace(id: Section): string {
  return placeNamed(id)?.name ?? id;
}
