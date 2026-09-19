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

/** Calls whose key no reading of the source can list. Only falls: a key built
 *  from a finite set belongs in a map of literals the judge can read. */
const KEYS_NOBODY_CAN_EXPAND_TODAY = 6;

const sources = import.meta.glob(["./**/*.ts", "./**/*.tsx", "!./**/*.test.*", "!./i18n.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** The first argument of the call opening at `from`, balanced over brackets and strings. */
export function firstArgument(text: string, from: number): string {
  let depth = 0;
  let at = from;
  while (at < text.length) {
    const letter = text[at];
    if (letter === '"' || letter === "'" || letter === "`") {
      const closing = text.indexOf(letter, at + 1);
      at = closing === -1 ? text.length : closing + 1;
      continue;
    }
    if ("([{".includes(letter)) depth += 1;
    if (")]}".includes(letter)) {
      if (depth === 0) break;
      depth -= 1;
    }
    if (letter === "," && depth === 0) break;
    at += 1;
  }
  return text.slice(from, at).trim();
}

/** The string values of `const NAME … = { … }` in the same file, or `null` if any is not a literal. */
function valuesOfMap(text: string, name: string): string[] | null {
  const declared = new RegExp(`const ${name}\\b[^=]*=\\s*\\{([^}]*)\\}`).exec(text);
  if (!declared) return null;
  const entries = declared[1].split(",").map((one) => one.trim()).filter(Boolean);
  const values = entries.map((entry) => /:\s*"([^"]+)"$/.exec(entry)?.[1] ?? null);
  return values.every((value) => value !== null) ? (values as string[]) : null;
}

/** Every key a call can ask for, or `null` when the source does not say. */
export function expand(argument: string, text: string): string[] | null {
  const literal = /^"([^"]+)"$/.exec(argument);
  if (literal) return [literal[1]];
  const choice = /^[^?]+\?\s*"([^"]+)"\s*:\s*"([^"]+)"$/.exec(argument);
  if (choice) return [choice[1], choice[2]];
  const lookup = /^([A-Z][A-Z0-9_]*)\[[^\]]+\]$/.exec(argument);
  if (lookup) return valuesOfMap(text, lookup[1]);
  return null;
}

interface Asked {
  keys: string[];
  unexpanded: string[];
}

function asked(): Asked {
  const keys: string[] = [];
  const unexpanded: string[] = [];
  for (const [path, text] of Object.entries(sources)) {
    const source = text as string;
    for (const call of source.matchAll(/(?<![\w.$])t\(/g)) {
      const argument = firstArgument(source, (call.index ?? 0) + 2);
      const found = expand(argument, source);
      if (found === null) unexpanded.push(`${path}: t(${argument})`);
      else keys.push(...found.map((key) => `${path}: ${key}`));
    }
  }
  return { keys, unexpanded };
}

/** The object literal of one place, as written in `places.ts`. */
function declarationOf(id: string): string {
  const opened = placesSource.indexOf(`id: "${id}"`);
  expect(opened, `no place «${id}» is declared`).toBeGreaterThan(-1);
  return placesSource.slice(opened, placesSource.indexOf("}", opened));
}

describe("what the window asks the catalogue for", () => {
  test("the control: literals, a choice of two and a map of literals expand; a template does not", () => {
    const text = 'const WORDS: Record<string, string> = { here: "a.here", all: "a.all" };';
    expect(expand('"a.key"', text)).toEqual(["a.key"]);
    expect(expand('mode === "here" ? "a.one" : "a.two"', text)).toEqual(["a.one", "a.two"]);
    expect(expand("WORDS[mode]", text)).toEqual(["a.here", "a.all"]);
    expect(expand("`a.${mode}`", text)).toBeNull();
    expect(expand("MISSING[mode]", text)).toBeNull();
    expect(firstArgument('t(x ? "a" : "b", { n: f(1, 2) })', 2)).toBe('x ? "a" : "b"');
  });

  test("EVERY KEY THE WINDOW ASKS FOR EXISTS IN ENGLISH, the expanded ones included", () => {
    const { keys } = asked();
    expect(Object.keys(sources).length, "the sources were not read").toBeGreaterThan(50);
    expect(keys.map((one) => one.split(": ")[1]), "the Flows modes are read from their map").toContain(
      "window.flows.mode.all",
    );
    expect(keys.filter((one) => !(one.split(": ")[1] in en))).toEqual([]);
  });

  test("A KEY THE JUDGE CANNOT EXPAND IS COUNTED AND NAMED, NEVER SKIPPED", () => {
    const { unexpanded } = asked();
    expect(unexpanded.length, `lower the seed or give these a map of literals:\n${unexpanded.join("\n")}`).toBe(
      KEYS_NOBODY_CAN_EXPAND_TODAY,
    );
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
