import { invoker } from "./engine";

export type AttentionKind = "unreadable" | "handed" | "cap_reached" | "engine_unreachable" | "terminal_dead";

export type AttentionLink =
  | { kind: "tty"; tty: string }
  | { kind: "run"; run_id: string };

export interface AttentionRow {
  kind: AttentionKind | string;
  run_id?: string;
  step_id?: string;
  tty?: string;
  status_word: string;
  reason: string;
  since?: number | null;
  link?: AttentionLink | null;
}

export async function attentionQueue(): Promise<AttentionRow[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to ask");
  return invoke<AttentionRow[]>("attention_queue");
}

export function kindRank(kind: string): number {
  switch (kind) {
    case "unreadable":
      return 0;
    case "handed":
      return 1;
    case "cap_reached":
      return 2;
    case "engine_unreachable":
      return 3;
    case "terminal_dead":
      return 4;
    default:
      return 5;
  }
}

export function rankAttentionRows(rows: AttentionRow[]): AttentionRow[] {
  return [...rows].sort((a, b) => {
    const rA = kindRank(a.kind);
    const rB = kindRank(b.kind);
    if (rA !== rB) return rA - rB;
    if (a.since != null && b.since != null) return a.since - b.since;
    if (a.since != null) return -1;
    if (b.since != null) return 1;
    return 0;
  });
}

