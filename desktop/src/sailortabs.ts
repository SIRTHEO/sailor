/**
 * The screens this machine holds, as a list. Data and not a component: the
 * column reads it to build its third ground, and a `.ts` that pulls a screen in
 * pulls React with it — `world.test.ts` runs without a document and stopped at
 * `self is not defined`.
 */
export type SailorTab =
  | "keeps"
  | "cando"
  | "engines"
  | "profiles"
  | "models"
  | "equipment"
  | "commands"
  | "look";

export const SAILOR_TABS: { id: SailorTab; name: string; about: string; group: string }[] = [
  { id: "keeps", name: "What it keeps", about: "every store, its path and its size", group: "itself" },
  { id: "cando", name: "What it can do", about: "the actions a flow may use", group: "itself" },
  { id: "engines", name: "Engines", about: "which command lines are here, signed in, and how full", group: "setup" },
  { id: "profiles", name: "Profiles", about: "which account each command line runs under", group: "setup" },
  { id: "models", name: "Models", about: "the catalogue, and which is in use", group: "setup" },
  { id: "equipment", name: "Equipment", about: "tools, skills and rules on this machine", group: "setup" },
  { id: "commands", name: "Commands", about: "every verb sailor answers to", group: "setup" },
  { id: "look", name: "Appearance", about: "night, day, or whatever this machine says", group: "setup" },
];
