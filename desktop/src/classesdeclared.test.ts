/**
 * **A CLASS NOBODY STYLED LOOKS EXACTLY LIKE ONE NOBODY WROTE.** No rule
 * matches, nothing fails, the element just comes out bare — `Projects.tsx`
 * shipped with four. Both sides are read from the files themselves, so neither
 * can drift from a list kept here.
 */
import { describe, expect, test } from "vitest";
import { parseStylesheet } from "./contrast";
import stylesheetSource from "./styles.css?raw";

const sources = import.meta.glob("./*.tsx", { query: "?raw", import: "default", eager: true });

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

describe("the sheet and the screens", () => {
  test("EVERY CLASS THE WINDOW WEARS IS ONE THE SHEET DRESSES", () => {
    const inSheet = declared();
    const inCode = used();

    // THE CONTROL FIRST, both sides: an empty parse on either would make the
    // comparison pass for having compared nothing.
    expect(inSheet.size, "no class parsed out of the stylesheet").toBeGreaterThan(50);
    expect(inCode.size, "no component file parsed").toBeGreaterThan(3);

    const orphans: string[] = [];
    for (const [path, names] of inCode) {
      for (const name of names) {
        if (!inSheet.has(name)) orphans.push(`${path}: ${name}`);
      }
    }
    expect(orphans, "classes worn by an element and dressed by no rule").toEqual([]);
  });
});
