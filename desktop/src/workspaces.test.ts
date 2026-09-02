/**
 * **THE TWO SIDES OF ONE CONTRACT, COMPARED.** The window reads fields a Rust
 * struct writes; a rename on either side compiles on both and breaks only when
 * someone opens the screen. Here the field names are read out of the Rust file
 * itself, so the test cannot drift from what is actually shipped.
 */
import { describe, expect, test } from "vitest";
import { since } from "./workspaces";

describe("the project list contract", () => {
  test("EVERY FIELD THE WINDOW READS IS WRITTEN BY THE COMMAND", () => {
    const sorgenti = import.meta.glob("../src-tauri/src/workspaces.rs", { query: "?raw", import: "default", eager: true });
    const rust = Object.values(sorgenti)[0] as string;
    const struct = rust.slice(rust.indexOf("struct Project"), rust.indexOf("\n}", rust.indexOf("struct Project")));
    const written = [...struct.matchAll(/^\s{4}(\w+):/gm)].map((m) => m[1]);

    // THE CONTROL FIRST: if the parse found nothing, every `toContain` below
    // would pass against an empty list and the test would guard nothing.
    expect(written.length, "no fields parsed out of the Rust struct").toBeGreaterThan(3);

    for (const field of ["root", "name", "first_seen", "last_seen", "standing", "current"]) {
      expect(written, `the window reads «${field}» and the command does not write it`).toContain(field);
    }
  });
});

describe("how long ago", () => {
  test("the words a person uses, at every scale", () => {
    const now = 1_000_000;
    expect(since(now - 10, now)).toBe("just now");
    expect(since(now - 600, now)).toBe("10 min ago");
    expect(since(now - 7_200, now)).toBe("2 h ago");
    expect(since(now - 259_200, now)).toBe("3 d ago");
    expect(since(now - 5_184_000, now)).toBe("2 mo ago");
  });

  test("A CLOCK THAT RUNS BACKWARDS READS AS NOW, not as a negative age", () => {
    const now = 1_000_000;
    expect(since(now + 5_000, now)).toBe("just now");
  });
});
