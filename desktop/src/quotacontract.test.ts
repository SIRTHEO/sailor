/**
 * **THE TWO SIDES OF ONE CONTRACT, COMPARED**, both read from the files. The
 * window's fields and the Rust struct's are matched name by name, and the
 * words for a quota are checked apart: they are the ones easiest to get wrong.
 */
import { describe, expect, test } from "vitest";
import { perMillion, windowName } from "./quota";

const sorgenti = import.meta.glob("../src-tauri/src/models.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./quota.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the quota contract", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    const pairs = [
      { what: "Window", written: fields(rust, "struct Window", 4), read: fields(ts, "interface Window", 2) },
      { what: "Priced", written: fields(rust, "struct Priced", 4), read: fields(ts, "interface Priced", 2) },
      { what: "Choice", written: fields(rust, "struct Choice", 4), read: fields(ts, "interface Choice", 2) },
    ];
    for (const { what, written, read } of pairs) {
      // THE CONTROL FIRST, both sides.
      expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(2);
      expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(2);
      for (const field of read) {
        expect(written, `the window reads «${field}» and ${what} does not write it`).toContain(field);
      }
      for (const field of written) {
        expect(read, `${what} writes «${field}» and the window names it nowhere`).toContain(field);
      }
    }
  });
});

describe("the words for a quota", () => {
  test("A WINDOW NOBODY HAS A NAME FOR IS STILL SHOWN", () => {
    expect(windowName("five_hour")).toBe("5 hours");
    expect(windowName("seven_day")).toBe("7 days");
    // The provider adds windows: an unknown one must appear under its own key,
    // never vanish. A dropped row reads as «you have no such limit».
    expect(windowName("thirty_day")).toBe("thirty day");
  });

  test("NO PRICE, FREE AND CHEAP ARE THREE DIFFERENT ANSWERS", () => {
    expect(perMillion(null)).toBe("no price");
    expect(perMillion(0)).toBe("free");
    // The one that matters: rounded to two places this reads «$0.00», which a
    // person reads as free — and free is a different thing from nearly free.
    expect(perMillion(0.0015)).toBe("$0.0015");
    expect(perMillion(3)).toBe("$3.00");
  });
});
