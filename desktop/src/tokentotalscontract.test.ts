/**
 * **THREE HALF-COPIES OF ONE SHAPE**, and each had lost fields the wire
 * carries: turns, and the total engines declare without splitting it. A field
 * nobody declares cannot be read, and nothing goes red.
 */
import { describe, expect, test } from "vitest";

const engines = import.meta.glob("../../crates/ui/src/dashboard.rs", { query: "?raw", import: "default", eager: true });
const windows = import.meta.glob("./flow.ts", { query: "?raw", import: "default", eager: true });
const engine = Object.values(engines)[0] as string;
const window_ = Object.values(windows)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(?:pub )?(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the token totals contract", () => {
  test("EVERY NUMBER THE ENGINE SUMS IS A NUMBER THE WINDOW DECLARES", () => {
    const written = fields(engine, "pub struct TokenTotals", 4);
    const read = fields(window_, "export interface TokenTotals", 2);
    // The control first: a parse finding nothing would pass both loops.
    expect(written.length, "no fields parsed out of Rust").toBeGreaterThan(5);
    expect(read.length, "no fields parsed out of the window").toBeGreaterThan(5);
    for (const field of written) {
      expect(read, `the engine sums «${field}», the window names it nowhere`).toContain(field);
    }
    for (const field of read) {
      expect(written, `the window reads «${field}», the engine sums no such thing`).toContain(field);
    }
  });

  test("AND THE WINDOW KEEPS ONE COPY OF IT, not one per screen", () => {
    const shapes = {
      ...import.meta.glob("./*.ts", { query: "?raw", import: "default", eager: true }),
      ...import.meta.glob("./*.tsx", { query: "?raw", import: "default", eager: true }),
    };
    const inline: string[] = [];
    for (const [path, source] of Object.entries(shapes)) {
      // A TYPE, not a value: the fields typed `number` are a shape written out
      // by hand, while the same names set to zero are only a fixture.
      if (path.endsWith("flow.ts")) continue;
      if (/\{[^{}]*calls_without_tokens:\s*number[^{}]*\}/.test(source as string)) {
        inline.push(path);
      }
    }
    expect(inline, "these spell the totals out instead of naming TokenTotals").toEqual([]);
  });
});
