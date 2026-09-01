import { describe, expect, test } from "vitest";

/**
 * **TWO LISTS ON ONE CLOSED TYPE DIVERGE BY CONSTRUCTION, not with time.**
 * Sometimes they are born apart, and that case leaves no trace in any history:
 * no diff shows it, no date explains it, each half is internally right. What
 * finds it is looking for the shape instead of looking for the defect.
 */

const sources = import.meta.glob("./*.{ts,tsx}", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

const DECLARATION = /const\s+(\w+)\s*:\s*(?:Partial<)?Record<\s*(\w+)\s*,\s*([^=]{1,40}?)>?\s*=/g;

interface Declared {
  file: string;
  name: string;
  keyType: string;
  valueType: string;
}

/** Every hand-written `Record<Type, …>` in this folder, outside the tests. */
export function declarations(files: Record<string, string>): Declared[] {
  const found: Declared[] = [];
  for (const [path, text] of Object.entries(files)) {
    const file = path.replace("./", "");
    if (file.includes(".test.")) continue;
    for (const m of text.matchAll(DECLARATION)) {
      found.push({ file, name: m[1], keyType: m[2], valueType: m[3].trim().replace(/>$/, "") });
    }
  }
  return found;
}

const DRAWN = /ReactNode|ReactElement|JSX\.Element/;

describe("one species, one drawing", () => {
  /**
   * `KIND_ICON` and `MARK` were both `Record<StepKind, …>` of JSX, in two
   * files, and nine kinds out of nine were drawn differently: a species read as
   * two pictures, so a mark learned in the toolbox was not on the board.
   */
  test("NO TYPE IS DRAWN TWICE, in two places and two visual languages", () => {
    const drawn = declarations(sources).filter((d) => DRAWN.test(d.valueType));
    const byType = new Map<string, Declared[]>();
    for (const d of drawn) byType.set(d.keyType, [...(byType.get(d.keyType) ?? []), d]);
    const twice = [...byType.entries()].filter(([, list]) => list.length > 1);
    expect(
      twice.map(([type, list]) => `${type}: ${list.map((d) => `${d.file}.${d.name}`).join(" e ")}`),
      "due mappe di disegni sullo stesso tipo chiuso: nasceranno diverse, e nessun diff lo mostrerà",
    ).toEqual([]);
  });

  /**
   * **CHI MISURA VA MISURATO.** If the pattern stopped matching, the list would
   * be empty and the check above would pass by having looked at nothing — the
   * failure a ratchet cannot feel, because finding less is what it allows.
   */
  test("the search still finds the maps it is meant to compare", () => {
    const all = declarations(sources);
    expect(all.length).toBeGreaterThan(4);
    expect(all.map((d) => `${d.file}.${d.name}`)).toContain("StepNode.tsx.KIND_ICON");
    expect(all.some((d) => DRAWN.test(d.valueType))).toBe(true);
    expect(declarations({ "./finto.ts": "const X: Record<A, ReactNode> = {};" })).toHaveLength(1);
  });
});
