/**
 * **A KEY THE WINDOW ASKS FOR AND NOBODY WROTE COMES BACK AS THE KEY.** `t()`
 * falls back to its argument, so a missing entry is drawn on screen with no
 * red anywhere; and a place named by a literal stays English in every build.
 */
import { describe, expect, test } from "vitest";
import { placeNamed } from "./places";
import placesSource from "./places.ts?raw";
import enOnDisk from "../../i18n/en.json";
import itOnDisk from "../../i18n/it.json";

const en = enOnDisk as Record<string, string>;
const it = itOnDisk as Record<string, string>;

const sources = import.meta.glob(["./**/*.ts", "./**/*.tsx", "!./**/*.test.*"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** The object literal of one place, as written in `places.ts`. */
function declarationOf(id: string): string {
  const opened = placesSource.indexOf(`id: "${id}"`);
  expect(opened, `no place «${id}» is declared`).toBeGreaterThan(-1);
  return placesSource.slice(opened, placesSource.indexOf("}", opened));
}

describe("what the window asks the catalogue for", () => {
  test("EVERY LITERAL KEY PASSED TO t() EXISTS IN ENGLISH", () => {
    const missing: string[] = [];
    for (const [path, text] of Object.entries(sources)) {
      for (const match of (text as string).matchAll(/\bt\(\s*"([^"]+)"/g)) {
        if (!(match[1] in en)) missing.push(`${path}: ${match[1]}`);
      }
    }
    expect(Object.keys(sources).length, "the sources were not read").toBeGreaterThan(50);
    expect(missing).toEqual([]);
  });

  /**
   * **THE ENGLISH BUILD CANNOT TELL A LITERAL FROM AN ENTRY**: both read «Flows».
   * So the declaration itself is read, and both languages must hold the words.
   */
  test("THE FLOWS PLACE IS NAMED AND ASKS THROUGH t(), in both catalogues", () => {
    const declared = declarationOf("flows");
    expect(declared).toMatch(/name:\s*t\("window\.place\.flows\.name"\)/);
    expect(declared).toMatch(/asks:\s*t\("window\.place\.flows\.asks"\)/);
    expect(placeNamed("flows")?.name).toBe(en["window.place.flows.name"]);
    expect(it["window.place.flows.name"]).not.toBe(en["window.place.flows.name"]);
    expect(it["window.place.flows.asks"]).not.toBe(en["window.place.flows.asks"]);
  });
});
