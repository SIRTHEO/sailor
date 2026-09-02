/**
 * **THE TWO SIDES OF ONE CONTRACT**, and the four standings apart: they are the
 * point of the register, and the easiest thing to lose at the crossing.
 */
import { describe, expect, test } from "vitest";
import { STATUS_WORDS } from "./register";

const sorgenti = import.meta.glob("../src-tauri/src/faults.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./register.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the register contract", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    for (const { what, written, read } of [
      { what: "Entry", written: fields(rust, "struct Entry", 4), read: fields(ts, "interface Entry", 2) },
      { what: "Register", written: fields(rust, "struct Register", 4), read: fields(ts, "interface Register", 2) },
    ]) {
      expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(1);
      expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(1);
      for (const field of read) expect(written, `the window reads «${field}», ${what} does not write it`).toContain(field);
      for (const field of written) expect(read, `${what} writes «${field}», the window names it nowhere`).toContain(field);
    }
  });

  test("ALL FOUR STANDINGS CROSS, and three of them have prose to set", () => {
    const arms = [...rust.matchAll(/Standing::\w+ => "([a-z ]+)"/g)].map((m) => m[1]);
    expect(new Set(arms).size, "the standings did not parse").toBe(4);
    for (const standing of arms) {
      expect(ts, `«${standing}» is not in the window's Standing union`).toContain(`"${standing}"`);
    }
    // Three, not four: `unrecognised` is a reading, never a status to write —
    // offering it as a button would let somebody set a fault to «I do not
    // understand this», which is not a thing anybody means.
    expect(Object.keys(STATUS_WORDS).sort()).toEqual(["closed", "open", "partly closed"]);
  });
});
