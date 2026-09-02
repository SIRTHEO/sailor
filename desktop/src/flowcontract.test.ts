/**
 * **WHAT THE SHELL SENDS AND THE WINDOW DECLARES, COMPARED.** A field the shell
 * writes and the window does not name is not an error anywhere: it arrives, it
 * is dropped, and nothing turns red. `origin` — which of the three sources a
 * flow came from — travelled that way from the day it was added.
 */
import { describe, expect, test } from "vitest";

/** The Rust that composes what `flows` returns. */
function shell(): string {
  const sorgenti = import.meta.glob("../src-tauri/src/main.rs", {
    query: "?raw",
    import: "default",
    eager: true,
  });
  return Object.values(sorgenti)[0] as string;
}

/** The TypeScript that says what the window expects. */
function window_(): string {
  const sorgenti = import.meta.glob("./flow.ts", {
    query: "?raw",
    import: "default",
    eager: true,
  });
  return Object.values(sorgenti)[0] as string;
}

describe("the flow list contract", () => {
  test("EVERY FIELD THE SHELL SENDS IS NAMED BY THE WINDOW", () => {
    const rust = shell();
    const enumeration = rust.slice(rust.indexOf("enum FlowEntry"), rust.indexOf("\n}", rust.indexOf("enum FlowEntry")));
    const sent = [...new Set([...enumeration.matchAll(/^\s{8}(\w+):/gm)].map((m) => m[1]))];

    // THE CONTROL FIRST: a parse that found nothing would make the loop below
    // pass over an empty list, and the test would guard nothing at all.
    expect(sent, "no fields parsed out of the Rust enum").toContain("flow");
    expect(sent.length, "fewer fields than the enum plainly has").toBeGreaterThan(2);

    const declared = window_();
    for (const field of sent) {
      expect(
        declared,
        `the shell sends «${field}» in FlowEntry and flow.ts never names it`,
      ).toContain(field);
    }
  });
});
