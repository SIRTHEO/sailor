/**
 * **THE TWO SIDES OF ONE CONTRACT, COMPARED**, both read from the files: the
 * fields the window names and the fields the sweep writes.
 */
import { describe, expect, test } from "vitest";
import { PRESENCE_WORD } from "./machine";

const sorgenti = import.meta.glob("../src-tauri/src/tools.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./machine.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the machine sweep contract", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    const pairs = [
      { what: "Tool", written: fields(rust, "struct Tool", 4), read: fields(ts, "interface Tool", 2) },
      { what: "BadLine", written: fields(rust, "struct BadLine", 4), read: fields(ts, "interface BadLine", 2) },
      { what: "Sweep", written: fields(rust, "struct Sweep", 4), read: fields(ts, "interface Sweep", 2) },
    ];
    for (const { what, written, read } of pairs) {
      expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(1);
      expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(1);
      for (const field of read) {
        expect(written, `the window reads «${field}» and ${what} does not write it`).toContain(field);
      }
      for (const field of written) {
        expect(read, `${what} writes «${field}» and the window names it nowhere`).toContain(field);
      }
    }
  });

  test("EVERY STATE THE ENGINE CAN ANSWER HAS A WORD", () => {
    // The words are read off the Rust match arms, so a fourth state added
    // there fails here instead of arriving on screen as `undefined`.
    const arms = [...rust.matchAll(/Presence::\w+\(_\) => "(\w+)"/g)].map((m) => m[1]);
    expect(new Set(arms).size, "no presence arms parsed").toBe(3);
    for (const state of new Set(arms)) {
      expect(Object.keys(PRESENCE_WORD), `«${state}» has no word on screen`).toContain(state);
    }
  });
});
