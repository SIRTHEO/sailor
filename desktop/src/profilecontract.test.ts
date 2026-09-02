/**
 * **THE TWO SIDES OF ONE CONTRACT, COMPARED** — and neither side is copied
 * here. The field names are read out of the Rust file and out of `profiles.ts`,
 * so a third list cannot go stale on its own while both real ones move.
 */
import { describe, expect, test } from "vitest";

const sorgenti = import.meta.glob("../src-tauri/src/profiles.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./profiles.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the profile contract", () => {
  test("EVERY FIELD THE WINDOW READS IS WRITTEN BY THE COMMAND", () => {
    const pairs = [
      { what: "CommandLine", written: fields(rust, "struct CommandLine", 4), read: fields(ts, "interface CommandLine", 2) },
      { what: "Row", written: fields(rust, "struct Row", 4), read: fields(ts, "interface Row", 2) },
    ];

    for (const { what, written, read } of pairs) {
      // THE CONTROL FIRST: a parse that found nothing would make every check
      // below pass against an empty list, and the test would guard nothing.
      expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(2);
      expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(2);

      for (const field of read) {
        expect(written, `the window reads «${field}» and ${what} does not write it`).toContain(field);
      }
      // AND THE OTHER DIRECTION: a field the engine writes and nobody reads is
      // how `origin` reached the window and was dropped in silence.
      for (const field of written) {
        expect(read, `${what} writes «${field}» and the window names it nowhere`).toContain(field);
      }
    }
  });

  test("THE STATES ARE NOT NARROWED ON THE WAY", () => {
    // Every word the Rust side can answer with has to exist in the union the
    // window declares: one missing would be a state that types as impossible
    // and arrives anyway.
    for (const word of ["supported", "not supported", "unverified"]) {
      expect(ts, `«${word}» is not in the window's Native union`).toContain(`"${word}"`);
    }
    for (const word of ["yes", "no", "not known", "home does not move"]) {
      expect(ts, `«${word}» is not in the window's Access union`).toContain(`"${word}"`);
    }
    for (const word of ["variable", "symlink", "none"]) {
      expect(ts, `«${word}» is not in the window's Mechanism union`).toContain(`"${word}"`);
    }
  });
});
