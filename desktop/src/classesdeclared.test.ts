/**
 * **A CLASS NOBODY STYLED LOOKS EXACTLY LIKE ONE NOBODY WROTE.** No rule
 * matches, nothing fails, the element just comes out bare — `Projects.tsx`
 * shipped with four. Both sides are read from the files themselves, so neither
 * can drift from a list kept here.
 */
import { describe, expect, test } from "vitest";
import { parseStylesheet } from "./contrast";
import { utilities } from "./tailwindcandidates";
import stylesheetSource from "./styles.css?raw";

// Every screen, wherever it sits: a folder is not a place to hide from this.
const sources = import.meta.glob("./**/*.tsx", { query: "?raw", import: "default", eager: true });

/**
 * Classes React Flow styles itself. The prefixed ones are a family; `nodrag`
 * and `nowheel` carry no prefix, so they are named **one by one**: exempting
 * the file would forgive whatever it grows later, the next orphan included.
 */
const THEIRS = new Set(["nodrag", "nowheel"]);

function ours(name: string): boolean {
  return !name.startsWith("react-flow") && !THEIRS.has(name);
}

/** Every class named in a literal `className="…"`, file by file. */
function used(): Map<string, Set<string>> {
  const byFile = new Map<string, Set<string>>();
  for (const [path, text] of Object.entries(sources)) {
    const names = new Set<string>();
    for (const match of (text as string).matchAll(/className="([^"{}]+)"/g)) {
      for (const name of match[1].split(/\s+/).filter(Boolean).filter(ours)) names.add(name);
    }
    if (names.size > 0) byFile.set(path, names);
  }
  return byFile;
}

/** Every class any selector in the sheet mentions. */
function declared(): Set<string> {
  const names = new Set<string>();
  for (const rule of parseStylesheet(stylesheetSource).rules) {
    for (const match of rule.selector.matchAll(/\.([A-Za-z0-9_-]+)/g)) names.add(match[1]);
  }
  return names;
}

/**
 * Bare `data-…` attributes are pure visual markers, so one the sheet never
 * names reaches nobody. Those written with a value (`data-here={…}`) are left
 * alone: they also serve tests as handles.
 */
function bareMarkers(): Map<string, Set<string>> {
  const byFile = new Map<string, Set<string>>();
  for (const [path, text] of Object.entries(sources)) {
    const names = new Set<string>();
    // Bare, and `={… || undefined}` too: that pattern is a boolean marker in
    // every way that matters — it is either on the element or absent — and it
    // is how `data-gone` sat on a row for a project that no longer exists
    // while looking exactly like a live one.
    for (const match of (text as string).matchAll(/\s(data-[a-z-]+)(?=[\s/>])/g)) names.add(match[1]);
    for (const match of (text as string).matchAll(/\s(data-[a-z-]+)=\{[^}]*\|\|\s*undefined\}/g)) names.add(match[1]);
    if (names.size > 0) byFile.set(path, names);
  }
  return byFile;
}

describe("the sheet and the screens", () => {
  test("A MARK WITH NO RULE BEHIND IT REACHES NOBODY", () => {
    const sheet = stylesheetSource;
    const marks = bareMarkers();

    // THE CONTROL FIRST: no marker parsed would make the loop below vacuous.
    expect([...marks.values()].reduce((n, set) => n + set.size, 0),
      "no bare data- marker parsed out of the components").toBeGreaterThan(0);

    const unseen: string[] = [];
    for (const [path, names] of marks) {
      for (const name of names) {
        if (!sheet.includes(`[${name}]`)) unseen.push(`${path}: ${name}`);
      }
    }
    expect(unseen, "markers written on an element that no rule ever looks at").toEqual([]);
  });

  /**
   * A hex in a component answers to no scheme. `STATE_COLOR` was a copy of the
   * six state roles, and when the ground went dark the minimap stayed lit from
   * noon: the only thing on screen still wearing the other palette.
   */
  test("NO COMPONENT NAMES A COLOUR OF ITS OWN", () => {
    // A brand's mark is not a role of this sheet: it is the brand's identity,
    // and it is the same tint under either ground.
    const BRANDS = "./ToolMark.tsx";
    const guilty: string[] = [];
    for (const [path, text] of Object.entries(sources)) {
      if (path === BRANDS || path.includes(".test.")) continue;
      for (const match of (text as string).matchAll(/#[0-9a-fA-F]{3}(?:[0-9a-fA-F]{3})?\b/g)) {
        guilty.push(`${path}: ${match[0]}`);
      }
    }
    // THE CONTROL: the brand file really does hold what it is exempted for.
    expect((sources[BRANDS] as string).match(/#[0-9a-fA-F]{6}/g)?.length ?? 0).toBeGreaterThan(4);
    expect(guilty, "a tint written into a component follows no scheme").toEqual([]);
  });

  test("EVERY CLASS THE WINDOW WEARS IS ONE THE SHEET DRESSES", async () => {
    const inSheet = declared();
    const inCode = used();
    const worn = [...inCode.values()].flatMap((names) => [...names]);
    // A utility is dressed by Tailwind, and only if it exists.
    const fromTailwind = await utilities(worn.filter((name) => !inSheet.has(name)));

    // THE CONTROL FIRST, on all three sides: an empty parse on any would make
    // the comparison pass for having compared nothing.
    expect(inSheet.size, "no class parsed out of the stylesheet").toBeGreaterThan(50);
    expect(inCode.size, "no component file parsed").toBeGreaterThan(3);
    expect(
      (await utilities(["flex", "bg-foreground", "justify-strat"])),
      "the compiler answers yes to everything, or to nothing",
    ).toEqual(new Set(["flex", "bg-foreground"]));

    const orphans: string[] = [];
    for (const [path, names] of inCode) {
      for (const name of names) {
        if (!inSheet.has(name) && !fromTailwind.has(name)) orphans.push(`${path}: ${name}`);
      }
    }
    expect(orphans, "classes worn by an element and dressed by no rule").toEqual([]);
  });
});
