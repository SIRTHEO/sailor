// What Sailor keeps, where it lives, and what is in service: the engine's
// answer, with paths a person can go and look at.

import { invoker } from "./engine";

export interface Store {
  what: string;
  where: string;
  how_many: number | null;
  bytes: number | null;
  exists: boolean;
  /** When the store came to be, in seconds since the epoch. */
  since: number | null;
}

export interface InService {
  binary: string | null;
  built_at: number | null;
  commit: string | null;
  window_version: string;
}

export interface Keeps {
  home: string;
  home_files: number;
  home_bytes: number;
  stores: Store[];
  in_service: InService;
  project_root: string | null;
}

export async function whatSailorKeeps(): Promise<Keeps> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: nothing to look at");
  return invoke<Keeps>("what_sailor_keeps");
}

/** Bytes as a person reads them, with one decimal past a kilobyte. */
export function sizeWords(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** A moment as a person reads it, in one format whatever machine renders it. */
export function whenWords(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString("en-GB", { hour12: false });
}

/** A count as a person reads it: grouped in threes, the way the design writes
 *  them, so six figures are scanned instead of counted. */
export function countWords(how_many: number): string {
  return how_many.toLocaleString("en-US").replace(/,/g, " ");
}
