// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { FlowMap } from "./FlowMap";
import { readFlowMap, type FlowFileOnDisk } from "./flowmapread";

/**
 * **THE FIVE QUESTIONS THE MAP EXISTS TO ANSWER**, asked of the painted DOM
 * and not of the reading behind it: a field computed right and never drawn
 * answers nobody. The loudest of them is a call naming a file nobody wrote.
 */

afterEach(cleanup);

function file(name: string, origin: "shipped" | "own", targets: string[]): FlowFileOnDisk {
  const steps = targets.map((flow, i) => ({ id: `call${String(i)}`, action: "subflow", with: { flow } }));
  return { name: `${name}.flow.json`, origin, text: JSON.stringify({ id: name, description: "", graph: { steps } }) };
}

const WHOLE = [
  file("caller", "shipped", ["called", "also-called"]),
  file("called", "shipped", []),
  file("also-called", "own", []),
  file("lonely", "own", []),
  file("ring-a", "own", ["ring-b"]),
  file("ring-b", "own", ["ring-a"]),
];

const HURT = [...WHOLE, file("hopeful", "own", ["a-flow-that-was-never-written"])];

function draw(files: FlowFileOnDisk[], onOpen?: (flow: string) => void) {
  render(<FlowMap ask={{ state: "read", reading: readFlowMap(files) }} onOpen={onOpen} />);
}

function plate(id: string): HTMLElement {
  const found = document.querySelector(`[data-flow="${id}"]`);
  expect(found, `no plate drawn for ${id}`).not.toBeNull();
  return found as HTMLElement;
}

describe("the map of the flows, as it is painted", () => {
  test("A CALL NAMING A FILE NOBODY WROTE IS IMPOSSIBLE TO MISS", () => {
    draw(HURT);
    const banner = document.querySelector("[data-broken]");
    expect(banner?.getAttribute("data-broken")).toBe("1");
    expect(banner?.textContent).toContain("hopeful");
    expect(banner?.textContent).toContain("a-flow-that-was-never-written");
    expect(plate("hopeful").textContent).toContain("calls nothing there");
  });

  test("AND WHEN THERE IS NONE THE MAP SAYS SO, RATHER THAN SAYING NOTHING", () => {
    draw(WHOLE);
    expect(document.querySelector("[data-broken]")).toBeNull();
    expect(screen.getByText(/No flow calls a flow that is not there/)).toBeTruthy();
  });

  test("A PLATE NAMES ITS CALLERS AND ITS CALLS WITHOUT BEING HOVERED", () => {
    draw(WHOLE);
    expect(plate("caller").textContent).toContain("calls →");
    expect(plate("caller").textContent).toContain("called");
    expect(plate("called").textContent).toContain("← called by");
    expect(plate("called").textContent).toContain("caller");
  });

  test("A FLOW NOTHING CALLS SAYS SO IN WORDS, NOT IN A TINT", () => {
    draw(WHOLE);
    expect(plate("caller").textContent).toContain("nothing calls it");
    expect(plate("called").textContent).not.toContain("nothing calls it");
    expect(screen.getByText(/On their own/)).toBeTruthy();
    expect(plate("lonely").textContent).toContain("nothing calls it");
  });

  test("WHAT SAILOR SHIPS AND WHAT IS YOURS ARE TOLD APART BY A WORD ON THE PLATE", () => {
    draw(WHOLE);
    expect(plate("caller").textContent).toContain("ships with Sailor");
    expect(plate("lonely").textContent).toContain("your own");
  });

  test("A RING IS DRAWN AS A ROUND TRIP THAT CLOSES", () => {
    draw(WHOLE);
    expect(screen.getByText(/^ring-a → ring-b → ring-a$/)).toBeTruthy();
    expect(plate("ring-a").textContent).toContain("in a ring");
    expect(plate("caller").textContent).not.toContain("in a ring");
  });

  test("CLICKING A FLOW ASKS FOR IT TO BE OPENED, AND CHANGES NOTHING HERE", () => {
    const asked: string[] = [];
    draw(WHOLE, (flow) => asked.push(flow));
    fireEvent.click(plate("called"));
    expect(asked).toEqual(["called"]);
  });

  test("A FILE THAT WOULD NOT PARSE IS CONFESSED ON THE SURFACE", () => {
    draw([...WHOLE, { name: "torn.flow.json", origin: "own", text: "{ not json" }]);
    expect(screen.getByText(/Not read, and so not on the map/).textContent).toContain("torn.flow.json");
  });

  test("STILL READING, BROKEN, AND NOTHING THERE EACH SAY WHICH THEY ARE", () => {
    render(<FlowMap ask={{ state: "reading" }} />);
    expect(screen.getByText(/Reading the flow files/)).toBeTruthy();
    cleanup();

    render(<FlowMap ask={{ state: "unreadable", why: "the engine answered nothing" }} />);
    expect(screen.getByText(/this map is empty and not true/)).toBeTruthy();
    expect(screen.getByText("the engine answered nothing")).toBeTruthy();
    cleanup();

    draw([]);
    expect(screen.getByText("No flow files here yet.")).toBeTruthy();
  });
});
