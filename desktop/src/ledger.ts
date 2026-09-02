// The ledger looked at as it is: its tables, and one statement a person types.
// Read-only by the engine's own guard, never by this side's good manners.

import { invoker } from "./engine";

export interface TableCount {
  name: string;
  rows: number;
}

export interface Tables {
  directory: string;
  exists: boolean;
  tables: TableCount[];
}

export interface Answer {
  columns: string[];
  rows: unknown[][];
  truncated: boolean;
}

export async function ledgerTables(): Promise<Tables> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to list");
  return invoke<Tables>("ledger_tables");
}

export async function ledgerQuery(sql: string): Promise<Answer> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to ask");
  return invoke<Answer>("ledger_query", { sql });
}

/** The statement the browser opens a table with. */
export function openingStatement(table: string): string {
  return `select * from ${table} order by 1 desc limit 200`;
}

/** A cell as text: `null` in italics is the screen's job, the word is this one's. */
export function cellText(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
}
