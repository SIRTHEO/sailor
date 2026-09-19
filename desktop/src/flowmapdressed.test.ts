/**
 * **THE CLASS JUDGE READS `className="…"` AND THIS FILE KEEPS ITS CLASSES IN
 * CONSTANTS**, so it would see none of them. `justify-strat` produces nothing
 * and looks exactly like a class that works. Every token this surface wears,
 * wherever it is written, is put to Tailwind here.
 */
import { describe, expect, test } from "vitest";
import { utilities } from "./tailwindcandidates";
import source from "./FlowMap.tsx?raw";

const ROLE = /var\(--[a-z0-9-]+\)/g;
const CLASS_STRINGS = /(?:className=\{?`|className="|const [A-Z]+ =\s*\n?\s*")([^"`]+)/g;

function worn(text: string): string[] {
  const names = new Set<string>();
  for (const match of text.matchAll(CLASS_STRINGS)) {
    for (const raw of match[1].split(/\s+/)) {
      const name = raw.replace(/\$\{[^}]*\}/g, "").trim();
      if (name !== "" && !name.includes("{") && !name.includes("$")) names.add(name);
    }
  }
  return [...names];
}

describe("what the map of the flows wears", () => {
  test("EVERY CLASS IT WEARS IS ONE TAILWIND ACTUALLY MAKES", async () => {
    const names = worn(source);
    expect(names.length, "no class parsed out of the surface").toBeGreaterThan(20);
    expect(
      await utilities(["flex", "bg-foreground", "justify-strat"]),
      "the compiler answers yes to everything, or to nothing",
    ).toEqual(new Set(["flex", "bg-foreground"]));
    const made = await utilities(names);
    expect(names.filter((name) => !made.has(name)), "classes that compile to nothing").toEqual([]);
  });

  test("EVERY COLOUR IT NAMES IS A ROLE OF THE SHEET, AND NONE IS A TINT OF ITS OWN", () => {
    expect(source.match(/#[0-9a-fA-F]{3,6}\b/g)).toBeNull();
    expect([...new Set(source.match(ROLE) ?? [])].length).toBeGreaterThan(5);
  });
});
