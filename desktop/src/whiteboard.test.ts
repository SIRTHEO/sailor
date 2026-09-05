import { describe, expect, test } from "vitest";
import { loadSketch, newBlock, saveSketch, sketchText, type Block } from "./whiteboard";

describe("the whiteboard as text", () => {
  const blocks: Block[] = [
    { ...newBlock("a", "trigger"), text: "the person writes a path" },
    { ...newBlock("b", "engine"), text: "translate it", after: ["a"] },
    { ...newBlock("c", "human"), text: "write it over, or discard", after: ["b"] },
  ];

  /** EVERY BLOCK AND EVERY ARROW IS IN THE TEXT, numbered as drawn: the author
   *  reads nothing else. */
  test("every block and every arrow reaches the author, numbered as drawn", () => {
    const text = sketchText(blocks);
    expect(text).toContain("3 block(s), 2 arrow(s)");
    expect(text).toContain("Block 1 (trigger): the person writes a path");
    expect(text).toContain("Block 2 (engine): translate it");
    expect(text).toContain("Block 3 (human): write it over, or discard");
    expect(text).toContain("Arrow: 1 -> 2");
    expect(text).toContain("Arrow: 2 -> 3");
  });

  test("an arrow from a block that is gone is not drawn", () => {
    const orphan: Block[] = [{ ...newBlock("b", "engine"), text: "alone", after: ["a"] }];
    expect(sketchText(orphan)).toContain("1 block(s), 0 arrow(s)");
    expect(sketchText(orphan)).not.toContain("Arrow");
  });

  test("a block with no words says so instead of vanishing", () => {
    expect(sketchText([newBlock("a", "check")])).toContain("Block 1 (check): (no words)");
  });

  test("the sketch survives in storage, and a broken storage gives an empty board", () => {
    const kept = new Map<string, string>();
    const storage = { getItem: (key: string) => kept.get(key) ?? null, setItem: (key: string, value: string) => void kept.set(key, value) };
    saveSketch(storage, blocks);
    expect(loadSketch(storage)).toEqual(blocks);
    expect(loadSketch({ getItem: () => "not json" })).toEqual([]);
    expect(loadSketch(null)).toEqual([]);
  });
});
