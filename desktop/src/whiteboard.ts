// The whiteboard as data: blocks with a kind and words, and arrows between
// them. What the person draws is handed to `draft-a-flow` as text, and the
// text is the contract — the author reads blocks, words and arrows, nothing
// else.

import type { StepKind } from "./flow";

export interface Block {
  id: string;
  kind: StepKind;
  text: string;
  /** The ids of the blocks this one comes after: the arrows, drawn backwards. */
  after: string[];
}

/** The kinds a person draws with. `wait` and `branch` are the engine's own. */
export const SKETCH_KINDS: StepKind[] = ["trigger", "engine", "check", "deposit", "gesture", "human", "subflow"];

export function newBlock(id: string, kind: StepKind = "engine"): Block {
  return { id, kind, text: "", after: [] };
}

/** A block's number on the board, one-based, in drawing order. */
export function numberOf(blocks: Block[], id: string): number {
  return blocks.findIndex((block) => block.id === id) + 1;
}

/**
 * The sketch as the author reads it: one line per block with its number, its
 * kind and its words; one line per arrow. Arrows to a block that is gone are
 * not drawn.
 */
export function sketchText(blocks: Block[]): string {
  const arrows = blocks.flatMap((block) =>
    block.after
      .filter((from) => blocks.some((other) => other.id === from))
      .map((from) => `Arrow: ${numberOf(blocks, from)} -> ${numberOf(blocks, block.id)}`),
  );
  const lines = [
    `WHITEBOARD — ${blocks.length} block(s), ${arrows.length} arrow(s).`,
    ...blocks.map((block, index) => `Block ${index + 1} (${block.kind}): ${block.text.trim() || "(no words)"}`),
    ...arrows,
  ];
  return lines.join("\n");
}

export const SKETCH_STORAGE_KEY = "sailor.sketch";

export function loadSketch(storage: Pick<Storage, "getItem"> | null): Block[] {
  try {
    const raw = storage?.getItem(SKETCH_STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as Block[]) : [];
  } catch {
    return [];
  }
}

export function saveSketch(storage: Pick<Storage, "setItem"> | null, blocks: Block[]): void {
  try {
    storage?.setItem(SKETCH_STORAGE_KEY, JSON.stringify(blocks));
  } catch {
    // a private window, or storage denied: the sketch lives in memory only
  }
}
