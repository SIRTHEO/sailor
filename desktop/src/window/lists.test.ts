import { describe, expect, test } from "vitest";
import { LISTS, listEntry } from "./lists";

/**
 * **FIVE, AND THE COLUMN IS NOT THE ROW.** `primarynav.test.ts` holds the row
 * of places to three, because a row of words becomes a list at the fourth.
 * A column of icons is scanned, not read — ADR-026 — and it is held here
 * instead: named, so a regression says which entry broke the count.
 */
describe("the column of lists", () => {
  test("it carries these five, in this order", () => {
    expect(LISTS.map((entry) => entry.id)).toEqual([
      "workspaces",
      "flows",
      "terminals",
      "data",
      "keys",
    ]);
  });

  test("EVERY ENTRY SAYS ITS NAME, and none of them shows a raw key", () => {
    for (const entry of LISTS) {
      expect(entry.name, `${entry.id} has no sentence`).not.toContain("window.rail.");
      expect(entry.name.trim().length, `${entry.id} is nameless`).toBeGreaterThan(0);
    }
  });

  test("every entry has a mark, because the column is icons", () => {
    expect(LISTS.filter((entry) => entry.icon === undefined || entry.icon === null)).toEqual([]);
  });

  test("a place nobody declared refuses, instead of drawing nothing", () => {
    expect(listEntry("data").id).toBe("data");
    // @ts-expect-error the type forbids it; a build that loses the type must not
    // fall back to an empty button.
    expect(() => listEntry("history")).toThrow();
  });
});
