// The home lab, read the same way any other engine is: does it answer, what
// is loaded, how long did it take. Nothing here invokes a model — it reads
// `lab-status`, a collection the lab's own `lab-status` flow writes by timing
// the router's `GET /models` alone. See rapporto-laboratorio.md, "Per la tab
// UI", for the row's exact shape; this file trusts that shape and no other.

import { ledgerQuery } from "./ledger";

export interface LabStatus {
  reachable: boolean;
  resident_models: string[];
  response_time_secs: number | null;
  checked_at: string;
  error: string | null;
}

const LATEST_LAB_STATUS_SQL =
  "select value from store where collection = 'lab-status' order by written_at desc limit 1";

function isLabStatus(value: unknown): value is LabStatus {
  if (typeof value !== "object" || value === null) return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.reachable === "boolean" &&
    Array.isArray(row.resident_models) &&
    (typeof row.response_time_secs === "number" || row.response_time_secs === null) &&
    typeof row.checked_at === "string"
  );
}

/** The most recent reading, or `null` when the lab has never been asked —
 * not the same as "not reachable", which is a reading that says so. */
export async function labStatus(): Promise<LabStatus | null> {
  const answer = await ledgerQuery(LATEST_LAB_STATUS_SQL);
  const cell = answer.rows[0]?.[0];
  if (cell === undefined || cell === null) return null;
  const parsed: unknown = typeof cell === "string" ? JSON.parse(cell) : cell;
  if (!isLabStatus(parsed)) throw new Error("the lab-status row does not carry the shape this window expects");
  return parsed;
}

export function residentWords(models: string[]): string {
  if (models.length === 0) return "nothing resident";
  return models.join(", ");
}

export function responseWords(secs: number | null): string {
  if (secs === null) return "response time unknown";
  return `${secs.toFixed(3)}s to answer`;
}
