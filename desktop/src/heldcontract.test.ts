/**
 * **THE TWO SIDES OF ONE CONTRACT**, both read from the files: what the ledger
 * command writes and what the window names.
 */
import { describe, expect, test } from "vitest";

const sorgenti = import.meta.glob("../src-tauri/src/ledger.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./held.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the ledger contract", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    const pairs = [
      { what: "Leftover", ts: "Leftover" },
      { what: "OpenRun", ts: "OpenRun" },
      { what: "Waiting", ts: "Waiting" },
      { what: "FailureClass", ts: "FailureClass" },
      { what: "Kept", ts: "Kept" },
      { what: "Held", ts: "Held" },
    ];
    for (const pair of pairs) {
      const written = fields(rust, `struct ${pair.what}`, 4);
      const read = fields(ts, `interface ${pair.ts}`, 2);
      expect(written.length, `no fields parsed out of Rust's ${pair.what}`).toBeGreaterThan(1);
      expect(read.length, `no fields parsed out of the window's ${pair.ts}`).toBeGreaterThan(1);
      for (const field of read) {
        expect(written, `the window reads «${field}», ${pair.what} does not write it`).toContain(field);
      }
      for (const field of written) {
        expect(read, `${pair.what} writes «${field}», the window names it nowhere`).toContain(field);
      }
    }
  });

  test("THE TALLY'S WINDOW IS A NUMBER SOMEBODY CHOSE, and it is named", () => {
    // Not «all of them»: a class that stopped happening two hundred runs ago
    // hides the one that started yesterday. The screen states the same number
    // it asks for, so a reader can tell what they are looking at.
    const chosen = rust.match(/const RECENT: usize = (\d+);/);
    expect(chosen, "the tally's window is not a named constant").not.toBeNull();
    const screen = import.meta.glob("./LedgerScreen.tsx", { query: "?raw", import: "default", eager: true });
    expect(
      Object.values(screen)[0] as string,
      `the screen does not say it is showing the last ${chosen?.[1]} runs`,
    ).toContain(`last ${chosen?.[1]} runs`);
  });
});
