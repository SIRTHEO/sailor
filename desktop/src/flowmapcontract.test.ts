/**
 * **THE TWO SIDES OF THE MAP'S CONTRACT, BOTH READ FROM THE FILES.** The shell
 * writes the flow files out and the window reads them back; a field renamed on
 * one side and not the other empties the map without a single test going red,
 * and an empty map reads exactly like a machine with no flows on it.
 */
import { describe, expect, test } from "vitest";
import { asTheMapReadsThem, origin } from "./flowmapask";
import { readFlowMap } from "./flowmapread";

const shell = import.meta.glob("../src-tauri/src/flows.rs", { query: "?raw", import: "default", eager: true });
const window_ = import.meta.glob("./flowmapask.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(shell)[0] as string;
const ts = Object.values(window_)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

describe("the contract of the flow map", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    const written = fields(rust, "struct FlowText", 4);
    const read = fields(ts, "interface FlowText", 2);

    // THE CONTROL FIRST, on both sides: two empty lists agree about nothing.
    expect(written.length, "no fields parsed out of Rust's FlowText").toBeGreaterThan(2);
    expect(read.length, "no fields parsed out of the window's FlowText").toBeGreaterThan(2);
    for (const field of read) {
      expect(written, `the window reads «${field}» and the shell does not write it`).toContain(field);
    }
    for (const field of written) {
      expect(read, `the shell writes «${field}» and the window names it nowhere`).toContain(field);
    }
  });

  test("THE COMMAND THE WINDOW CALLS IS ONE THE SHELL ANSWERS TO", () => {
    expect(ts).toContain('invoke<FlowText[]>("flow_texts")');
    expect(rust).toContain("pub(crate) fn flow_texts()");
  });

  test("THE WORD THE SHELL GIVES ITS OWN FLOWS IS THE ONE THE MAP SPLITS ON", () => {
    // Read from the source that owns it, so renaming «built in» in the engine
    // cannot leave the map quietly calling every shipped flow a person's own.
    const said = import.meta.glob("../../crates/flow/src/system.rs", {
      query: "?raw",
      import: "default",
      eager: true,
    });
    const engine = Object.values(said)[0] as string;
    const declared = /BUILTIN_ORIGIN: &str = "([^"]+)"/.exec(engine)?.[1];

    expect(declared, "the engine no longer names its own origin here").toBeDefined();
    expect(origin(declared as string)).toBe("shipped");
    expect(origin("yours")).toBe("own");
    expect(origin("the project's")).toBe("own");
  });
});

describe("what the window does with what it is handed", () => {
  test("A FILE THAT WOULD NOT OPEN IS NOT A FILE WITH NOTHING IN IT", () => {
    // Read as empty text it comes back as malformed JSON, which blames the
    // author for a permission, a broken link or a disk that answered no.
    const reading = readFlowMap(
      asTheMapReadsThem([
        { name: "shut.flow.json", origin: "yours", text: "", unreadable: "Permission denied (os error 13)" },
      ]),
    );

    expect(reading.nodes).toEqual([]);
    expect(reading.unread).toEqual([
      { name: "shut.flow.json", why: "Permission denied (os error 13)" },
    ]);
  });

  test("THE ORDER THE SHELL HANDS THEM IN IS THE ORDER THAT DECIDES WHICH RUNS", () => {
    const both = asTheMapReadsThem([
      { name: "a-flow.flow.json", origin: "built in", text: '{"id":"a-flow","description":"the shipped one","graph":{"steps":[]}}' },
      { name: "a-flow.flow.json", origin: "yours", text: '{"id":"a-flow","description":"yours","graph":{"steps":[]}}' },
    ]);
    const reading = readFlowMap(both);

    expect(reading.shadowed).toEqual(["a-flow"]);
    expect(reading.nodes).toHaveLength(1);
    expect(reading.nodes[0].origin, "the map named the copy that does not run").toBe("own");
  });
});
