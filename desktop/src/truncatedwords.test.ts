import { describe, expect, test } from "vitest";

/**
 * **A TRUNCATED WORD WITHOUT A TOOLTIP IS A WORD LOST.** Tailwind's `truncate`
 * cuts a string at the box's edge with no trace of what was cut; a person who
 * scans, per the owner's rule, reads the clipped half and never learns there
 * was a rest unless something on the same element offers it back. `Tooltip`
 * (from `@/components/ui/tooltip`) is that offer. This is rule 2 of w30's
 * five: `truncate` on an element, or one of its own JSX children, must sit
 * inside a `<Tooltip` somewhere in the same file's usage of that element —
 * checked per line, not per file, so one honest use elsewhere in the file
 * does not clear a dishonest one.
 *
 * Seed today: zero uses of `truncate` in `desktop/src` at all — measured
 * with a plain grep before this test existed. The check exists so the first
 * one to appear either carries its `Tooltip` or fails loudly, rather than
 * shipping silently the way every other truncation in this window's history
 * did before anyone asked.
 */

const sources = import.meta.glob("./*.tsx", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

/** Lines carrying a `truncate` class, in a file that never wraps it in
 * `Tooltip` at all — the cheap, certain half of the rule. A `truncate` whose
 * file also uses `Tooltip` earns closer reading, not an automatic pass: false
 * positives named below are for a person, not this function, to judge. */
export function truncatedWithoutTooltipFile(source: string): number[] {
  if (source.includes("Tooltip")) return [];
  const lines = source.split("\n");
  const hits: number[] = [];
  lines.forEach((line, index) => {
    if (/className\s*=\s*"[^"]*\btruncate\b/.test(line)) hits.push(index + 1);
  });
  return hits;
}

describe("truncated text names a Tooltip somewhere in its file", () => {
  test("every source file with `truncate` also uses `Tooltip`", () => {
    const offenders: string[] = [];
    for (const [path, source] of Object.entries(sources)) {
      const lines = truncatedWithoutTooltipFile(source);
      if (lines.length > 0) offenders.push(`${path}:${lines.join(",")}`);
    }
    // False positives this cheap check accepts on purpose: a `truncate` whose
    // Tooltip lives in a *different* file (a shared row component styled by
    // its caller) would still fail here and needs a person's judgment, not a
    // wider regex — named in w30 as the traded-off case.
    expect(offenders).toEqual([]);
  });

  test("the function itself: a file with no Tooltip and a truncate class is named", () => {
    const source = 'export const Row = () => <span className="truncate w-40">{x}</span>;';
    expect(truncatedWithoutTooltipFile(source)).toEqual([1]);
  });

  test("a file that also uses Tooltip is left to a person, not flagged here", () => {
    const source = [
      'import { Tooltip } from "@/components/ui/tooltip";',
      'export const Row = () => <span className="truncate w-40">{x}</span>;',
    ].join("\n");
    expect(truncatedWithoutTooltipFile(source)).toEqual([]);
  });
});
