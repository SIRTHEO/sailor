import { describe, expect, test } from "vitest";

/**
 * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. Two files that share a
 * basename across `.ts` and `.tsx` are one module to the compiler: it keeps one
 * and drops the other from the project without a word, and a case-insensitive
 * disk makes two spellings share a basename too. `tsc --noEmit` came back clean
 * while a whole component had never been read once — a gate green over code
 * nobody checked, which is the worst shape a gate can take.
 */
describe("what the compiler is given to read", () => {
  test("no two source files share a basename", () => {
    // Vite's own listing, so the check needs no node typings to run.
    const names = Object.keys(import.meta.glob("./*.{ts,tsx}")).map((path) => path.replace("./", ""));
    const seen = new Map<string, string>();
    const collisions: string[] = [];
    for (const name of names) {
      const flat = name.replace(/\.tsx?$/, "").toLowerCase();
      const first = seen.get(flat);
      if (first !== undefined) collisions.push(`${first} and ${name}`);
      else seen.set(flat, name);
    }
    expect(collisions, "the compiler reads only one of each pair").toEqual([]);
  });
});
