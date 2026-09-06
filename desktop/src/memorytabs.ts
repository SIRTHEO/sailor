/**
 * The views of what happened, as a list. Data and not a component, for the same
 * reason as `sailortabs.ts`: the machine's ground names one of them, and a
 * `.ts` that pulls a screen in pulls React with it.
 */
export type MemoryTab = "runs" | "spend" | "faults" | "ledger";

export const MEMORY_TABS: { id: MemoryTab; name: string; about: string }[] = [
  { id: "runs", name: "Runs", about: "open now, and every one before" },
  { id: "spend", name: "Spend and quota", about: "what it cost, what is left" },
  { id: "faults", name: "Faults", about: "and what would have prevented each" },
  { id: "ledger", name: "Ledger", about: "the tables, as they are" },
];
