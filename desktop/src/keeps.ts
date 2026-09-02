// What Sailor keeps, where it lives, and what is in service: the engine's
// answer, with paths a person can go and look at.

import { invoker } from "./engine";

export interface Store {
  what: string;
  where: string;
  how_many: number | null;
  bytes: number | null;
  exists: boolean;
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
