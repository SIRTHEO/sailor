import { describe, expect, test } from "vitest";
import { readFlowMap, type FlowFileOnDisk } from "./flowmapread";

function file(name: string, origin: "shipped" | "own", steps: unknown[]): FlowFileOnDisk {
  return { name: `${name}.flow.json`, origin, text: JSON.stringify({ id: name, description: name, graph: { steps } }) };
}

function calls(...targets: string[]): unknown[] {
  return targets.map((flow, i) => ({ id: `call${String(i)}`, action: "subflow", with: { flow } }));
}

const FIXTURES: FlowFileOnDisk[] = [
  file("one", "shipped", [{ id: "start", action: "trigger", with: {} }, ...calls("two", "three"), { id: "ghostly", action: "subflow", with: { flow: "ghost" } }]),
  file("two", "shipped", []),
  file("three", "own", [{ id: "each", action: "for_each", with: { flow: "two", items: [] } }]),
  file("lonely", "own", []),
  file("ring-a", "shipped", calls("ring-b")),
  file("ring-b", "own", calls("ring-a")),
];

describe("the map read off the flow files", () => {
  const reading = readFlowMap(FIXTURES);
  const node = (id: string) => reading.nodes.find((n) => n.id === id);

  test("A FLOW SAYS WHICH IT CALLS AND WHICH CALL IT", () => {
    expect(node("one")?.calls.map((c) => c.to)).toEqual(["ghost", "three", "two"]);
    expect(node("two")?.calledBy).toEqual(["one", "three"]);
    expect(node("three")?.calls.map((c) => c.via)).toEqual(["for_each"]);
  });

  test("A FLOW NOBODY CALLS IS NAMED AS ONE", () => {
    expect(reading.nodes.filter((n) => n.entry).map((n) => n.id)).toEqual(["lonely", "one", "ring-a"]);
    expect(node("lonely")?.calls).toEqual([]);
  });

  test("WHAT SAILOR SHIPS AND WHAT IS THE PERSON'S OWN ARE TOLD APART", () => {
    expect(node("one")?.origin).toBe("shipped");
    expect(node("lonely")?.origin).toBe("own");
    expect(reading.shipped).toBe(3);
    expect(reading.own).toBe(3);
  });

  test("A CYCLE COMES BACK AS A RING, NOT AS A TANGLE", () => {
    expect(reading.cycles).toEqual([["ring-a", "ring-b"]]);
    expect(node("ring-a")?.inCycle).toBe(true);
    expect(node("one")?.inCycle).toBe(false);
  });

  test("A SUBFLOW NAMING A FILE THAT IS NOT THERE IS THE LOUDEST THING READ", () => {
    expect(reading.broken).toEqual([{ from: "one", to: "ghost", step: "ghostly", via: "subflow" }]);
    expect(node("one")?.calls.find((c) => c.to === "ghost")?.missing).toBe(true);
    expect(node("one")?.calls.find((c) => c.to === "two")?.missing).toBe(false);
  });

  test("A FILE THAT IS NOT JSON IS REPORTED, NOT SWALLOWED", () => {
    const hurt = readFlowMap([...FIXTURES, { name: "torn.flow.json", origin: "own", text: "{ not json" }]);
    expect(hurt.unread.map((u) => u.name)).toEqual(["torn.flow.json"]);
    expect(hurt.nodes).toHaveLength(6);
  });

  test("EVERY FLOW SITS ON A LAYER BELOW WHOEVER CALLS IT", () => {
    expect(node("one")?.layer).toBe(0);
    expect(node("three")?.layer).toBe(1);
    expect(node("two")?.layer).toBe(2);
    expect(reading.nodes.every((n) => n.row >= 0)).toBe(true);
  });

  test("A FLOW OF YOUR OWN SHADOWS THE ONE SAILOR SHIPS UNDER THE SAME NAME", () => {
    const both = readFlowMap([file("twin", "shipped", []), file("twin", "own", calls("two")), ...FIXTURES]);
    expect(both.nodes.filter((n) => n.id === "twin")).toHaveLength(1);
    expect(both.nodes.find((n) => n.id === "twin")?.origin).toBe("own");
    expect(both.shadowed).toEqual(["twin"]);
  });

  test("A FLOW THAT CALLS ITSELF IS A RING OF ONE", () => {
    const self = readFlowMap([file("snake", "own", calls("snake"))]);
    expect(self.cycles).toEqual([["snake"]]);
    expect(self.nodes[0].inCycle).toBe(true);
    expect(self.broken).toEqual([]);
  });
});
