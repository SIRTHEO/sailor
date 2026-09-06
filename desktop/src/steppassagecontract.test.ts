/**
 * **A FIELD THE ENGINE RECORDS AND THE WINDOW CANNOT READ IS NOBODY'S ERROR**:
 * it crosses the wire, `invoke` hands back an object nothing declares, and
 * every check stays green. The reason is read from the crate that writes it,
 * not from the shell, which only clones the field.
 */
import { describe, expect, test } from "vitest";

const shells = import.meta.glob("../src-tauri/src/run.rs", { query: "?raw", import: "default", eager: true });
const engines = import.meta.glob("../../crates/flow/src/graph.rs", { query: "?raw", import: "default", eager: true });
const windows = import.meta.glob("./engine.ts", { query: "?raw", import: "default", eager: true });
const shell = Object.values(shells)[0] as string;
const engine = Object.values(engines)[0] as string;
const window_ = Object.values(windows)[0] as string;

function fields(source: string, opener: string, indent: number, closer = "\n}"): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf(closer, from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(?:pub )?(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

/** The struct's own fields, and the key its wire form adds. */
function reasonKeys(): string[] {
  const own = fields(engine, "struct Judgement", 4);
  const wire = fields(engine, "struct Written", 8, "\n    }");
  return [...new Set([...own, ...wire])];
}

function agree(written: string[], read: string[], what: string) {
  // THE CONTROL FIRST: a parse that found nothing would make both loops pass
  // over an empty list, and the test would guard nothing at all.
  expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(1);
  expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(1);
  for (const field of written) {
    expect(read, `${what} writes «${field}», the window names it nowhere`).toContain(field);
  }
  for (const field of read) {
    expect(written, `the window reads «${field}», ${what} does not write it`).toContain(field);
  }
}

describe("the step passage contract", () => {
  test("EVERY FIELD OF A PASSAGE IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    agree(fields(shell, "struct StepPassage", 4), fields(window_, "interface StepPassage", 2), "StepPassage");
  });

  test("AND SO IS EVERY FIELD OF THE REASON IT CARRIES", () => {
    const written = reasonKeys();
    expect(written, "the wire form of «found» is not parsed").toContain("found_was_there");
    agree(written, fields(window_, "interface Judgement", 2), "Judgement");
  });
});
