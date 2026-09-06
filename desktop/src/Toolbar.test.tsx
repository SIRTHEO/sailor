// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";

import App from "./App";
import stylesheetSource from "./styles.css?raw";
import { parseStylesheet } from "./contrast";
import { TOOL_GROUPS, TOOLBAR_KINDS, KINDS_WITH_ACTION, Toolbar } from "./Toolbar";
import { DEFAULT_ACTION_FOR_KIND, KNOWN_ACTIONS, type StepKind } from "./flow";
import { KIND_LABEL, KindIcon } from "./StepNode";

/**
 * A canvas with no flows is obtained by removing the sample data, which is what
 * somebody opening Sailor for the first time sees. No gesture in the window gets
 * to zero flows in jsdom: deleting them goes through the engine, absent here.
 */
const sample = vi.hoisted(() => ({ empty: false }));

vi.mock("./sample", async (importOriginal) => {
  const real = await importOriginal<typeof import("./sample")>();
  return {
    get SAMPLE() {
      return sample.empty ? [] : real.SAMPLE;
    },
    get SAMPLE_RUN() {
      return real.SAMPLE_RUN;
    },
  };
});

/**
 * **THE STEP TOOLBOX, INTERROGATED WHERE IT WOULD GO WRONG.** A screenshot at
 * one width cannot show that the commands and the paper share no pixel at
 * every width, or that every family offered creates a step with an action the
 * engine knows in both directions.
 */

afterEach(() => {
  cleanup();
  sample.empty = false;
});

// React Flow measures its own frame on mount: outside a real browser there is
// nobody to do it, and without these two the canvas does not mount at all.
class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

/** The window opens on «Now»: the canvas sits behind a place you must pick. */
function goToFlows(): void {
  fireEvent.click(screen.getByRole("button", { name: /^Board/ }));
}

function focusAFlow(container: HTMLElement): string {
  const item = container.querySelector("button.rail__item") as HTMLElement;
  const name = item.querySelector(".rail__label")?.textContent ?? "";
  fireEvent.click(item);
  return name;
}

/* ── the band, read from the stylesheet and from the tree ───────────────── */

const sheet = parseStylesheet(stylesheetSource);

/** The declarations of a rule; on an equal selector the last one wins. */
function declarationsOf(selector: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const rule of sheet.rules) {
    if (rule.selector !== selector) continue;
    for (const [property, value] of rule.declarations) found.set(property, value);
  }
  return found;
}

/**
 * What, declared on the band, would take it out of the column and back over
 * the paper. `position` is the whole family: with it the box leaves the flow,
 * and every offset below only says where it lands.
 */
const LIFTS_IT_OUT = ["position", "inset", "top", "right", "bottom", "left", "transform"];

/**
 * **THE BAND AND THE PAPER SHARE NO PIXEL.** jsdom lays nothing out, so the
 * empty intersection is not measured here: what is read are the two things
 * that make it empty at every width — the tree that keeps the boxes apart, and
 * the rule that stacks them.
 */
describe("the band the graph does not reach into", () => {
  test("THE BAR IS THE GRAPH'S SIBLING, NOT A TENANT OF ITS RECTANGLE", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar, "the bar is not drawn").not.toBeNull();
    const graph = container.querySelector(".react-flow") as HTMLElement;
    expect(graph, "the graph is not drawn").not.toBeNull();

    // Inside the graph the two rectangles are one rectangle, and whatever the
    // stylesheet then says about where in it the bar sits is a blind spot.
    expect(toolbar.closest(".react-flow"), "the bar is drawn inside the graph").toBeNull();
    expect(toolbar.closest(".canvas"), "the bar has left the canvas altogether").not.toBeNull();
    expect(
      toolbar.parentElement,
      "the bar and the graph are not two boxes of one column",
    ).toBe(graph.parentElement);

    // `Panel` marks what it places, so its return is legible in the tree even
    // before anything is laid out.
    expect(
      Array.from(toolbar.classList).filter((name) => name.startsWith("react-flow")),
      "the bar is a React Flow panel again",
    ).toEqual([]);
  });

  test("THE CANVAS STACKS THEM, AND IT IS THE GRAPH THAT GIVES GROUND", () => {
    const canvas = declarationsOf(".canvas");
    expect(canvas.get("display"), "the canvas does not lay its two boxes out").toBe("flex");
    expect(canvas.get("flex-direction"), "the two boxes sit side by side").toBe("column");

    // Without these three the band would be pushed out of the canvas instead
    // of taking its height from the graph: React Flow declares `height: 100%`,
    // which fills the column on its own.
    const graph = declarationsOf(".canvas > .react-flow");
    expect(graph.get("height"), "React Flow's own `height: 100%` still stands").toBe("auto");
    expect(graph.get("min-height"), "the graph cannot shrink below its content").toBe("0");
    expect(graph.get("flex"), "the graph neither takes the spare room nor gives it back").toMatch(
      /^1 1\b/,
    );

    // The growth of a wrapped band has to come off the graph, never off itself.
    expect(
      declarationsOf(".toolbar").get("flex-shrink"),
      "a taller band would be squeezed instead of shortening the paper",
    ).toBe("0");
  });

  test("THE BAR DECLARES NOTHING THAT WOULD LIFT IT BACK OVER THE PAPER", () => {
    const toolbar = declarationsOf(".toolbar");
    for (const property of LIFTS_IT_OUT) {
      expect(
        toolbar.get(property),
        `the bar declares «${property}»: out of the column, it can cover the graph again`,
      ).toBeUndefined();
    }

    // Ban 3 lends the one shadow to what «really does float above the canvas».
    // The band does not float, so the exemption is spent with the defect.
    expect(
      toolbar.get("box-shadow"),
      "the band still wears the floating bar's shadow",
    ).toBeUndefined();
  });

  test("the tools wrap, and that costs the graph height instead of hiding it", () => {
    // AND THERE ARE TWO, not one. The row's `flex-wrap` saves the case where
    // three groups do not fit side by side; the group's saves the case where a
    // whole group does not fit, and at 375px the band is narrower than a
    // three-tool group. Interrogating one selector only would let the other
    // through, and the tools past the edge would be unreachable.
    for (const selector of [".toolbar__row", ".toolbar__group"]) {
      expect(
        declarationsOf(selector).get("flex-wrap"),
        `«${selector}» does not wrap: its content spills out of the window sideways`,
      ).toBe("wrap");
    }
  });
});

describe("where the bar is", () => {
  test("IT IS AT THE CANVAS, AND NOT A PIECE OF THE COLUMN BESIDE IT", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar).not.toBeNull();
    expect(toolbar.closest(".rail")).toBeNull();
  });

  test("pressing a tool really adds a step to the focused flow", () => {
    // The tests below look at the bar on its own, with a fake in place of
    // `addStep`: from there a disconnected tool would look like it works. This
    // one crosses the whole window and reads the count the rail shows.
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const countOf = () => {
      const item = container.querySelector("button.rail__item") as HTMLElement;
      return item.querySelector(".rail__note")?.textContent ?? "";
    };
    const before = Number.parseInt(countOf(), 10);
    expect(Number.isFinite(before)).toBe(true);

    const check = container.querySelector(".toolbar__tool[data-kind='check']") as HTMLElement;
    fireEvent.click(check);

    expect(Number.parseInt(countOf(), 10)).toBe(before + 1);
  });

  test("the column no longer holds any tool", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);
    // The flows moved into the world column; the toolbox stayed on the canvas.
    const column = container.querySelector(".world") as HTMLElement;
    expect(column.querySelectorAll(".rail__item").length).toBeGreaterThan(0);
    expect(column.querySelectorAll(".toolbar__tool")).toHaveLength(0);
    expect(column.querySelectorAll(".palette__item")).toHaveLength(0);
  });
});

describe("what the bar offers", () => {
  test("EVERY FAMILY CREATES A STEP THE ENGINE RECOGNISES", () => {
    const seen: StepKind[] = [];
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={(kind) => seen.push(kind)} />,
    );

    for (const tool of Array.from(container.querySelectorAll<HTMLElement>(".toolbar__tool"))) {
      fireEvent.click(tool);
    }

    expect(seen).toEqual(TOOLBAR_KINDS);
    for (const kind of seen) {
      const action = DEFAULT_ACTION_FOR_KIND[kind];
      // It is not enough that the action exists: it must be one the engine's
      // vocabulary registers. Invented names create nodes that never save.
      expect(action, `the family «${kind}» has no action`).toBeDefined();
      expect(KNOWN_ACTIONS, `the action «${String(action)}» is not in the vocabulary`).toContain(action);
    }
  });

  test("the groups cover EXACTLY the families that have an action", () => {
    // In both directions: neither a tool with no action (a button that never
    // saves), nor an existing family left out of the toolbox with nobody
    // noticing. `wait` and `branch` have no action and stay out of both lists.
    expect([...TOOLBAR_KINDS].sort()).toEqual([...KINDS_WITH_ACTION].sort());
    expect(TOOLBAR_KINDS).toHaveLength(7);
  });

  test("every tool carries a mark AND a word", () => {
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={() => {}} />,
    );
    for (const tool of Array.from(container.querySelectorAll<HTMLElement>(".toolbar__tool"))) {
      // Ban 5 applied to shape: a mark on its own carries nothing, just as
      // colour on its own carries nothing.
      expect(tool.querySelector(".toolbar__mark svg")).not.toBeNull();
      expect(tool.querySelector(".toolbar__label")?.textContent?.trim()).toBeTruthy();
    }
  });

  test("AND THE MARK IS THE ONE ON THE CANVAS, not a second drawing of it", () => {
    const { container } = render(<Toolbar flowName="prima-corsa" onAdd={() => {}} />);
    for (const kind of TOOLBAR_KINDS) {
      const inBar = container.querySelector(`[data-kind="${kind}"] .toolbar__mark svg`);
      const onBoard = render(<KindIcon kind={kind} />).container.querySelector("svg");
      // The same drawing, compared as it lands in the document: two components,
      // two mount paths, one glyph. The day the bar grows its own again, the
      // markup stops matching here before anyone has to notice by eye.
      expect(inBar?.innerHTML, `«${KIND_LABEL[kind]}» is drawn twice`).toBe(onBoard?.innerHTML);
    }
  });

  test("every group names itself for whoever reads with a screen reader", () => {
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={() => {}} />,
    );
    const groups = Array.from(container.querySelectorAll<HTMLElement>("[role='group']"));
    expect(groups).toHaveLength(TOOL_GROUPS.length);
    for (const group of groups) {
      expect(group.getAttribute("aria-label")).toBeTruthy();
    }
  });

  /**
   * **THE NAME IS NOT DRAWN TWICE.** «Add to «x»» answered which of the many
   * lanes the step fell into; the board draws one flow and writes its name at
   * the top of the paper. The tie stays where it costs no pixel.
   */
  test("THE BAR NAMES ITS FLOW WHERE IT COSTS NOTHING, and does not draw it again", () => {
    const { container } = render(<Toolbar flowName="esamina-la-repo" onAdd={() => {}} />);

    const bar = container.querySelector(".toolbar") as HTMLElement;
    expect(bar.getAttribute("aria-label")).toContain("esamina-la-repo");
    expect(bar.textContent, "the name is drawn as well as spoken").not.toContain("esamina-la-repo");
  });

  /**
   * **THE BAR HAS NO SECOND FACE.** «No flow in focus» is not a state any more:
   * the board opens on a flow, and with none to open the empty canvas has the
   * screen. The prop that cannot be null is what makes it unreachable.
   */
  test("THE BAR IS ONLY EVER ABOUT A FLOW: no prompt, no empty face", () => {
    const { container } = render(<Toolbar flowName="prima-corsa" onAdd={() => {}} />);
    expect(container.querySelector(".toolbar__prompt")).toBeNull();
    expect(container.querySelectorAll(".toolbar__tool").length).toBeGreaterThan(0);
    expect(container.querySelectorAll("button:disabled")).toHaveLength(0);
  });
});

/**
 * **WITH ZERO FLOWS THE BAR IS NOT THERE AT ALL.** That moment belongs to the
 * empty canvas; two invitations on one screen cancel each other out.
 */
describe("with zero flows", () => {
  test("THE BAR GOES AND THE EMPTY CANVAS STAYS, not both", () => {
    sample.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank"), "the empty canvas is missing").not.toBeNull();
    expect(container.querySelector(".toolbar"), "the bar invites alongside the empty canvas").toBeNull();
  });

  test("with one flow the opposite holds: the bar is there and the empty canvas is not", () => {
    // In both directions, otherwise «no bar» would also be true of a bar that
    // never appears at all.
    const { container } = render(<App />);
    goToFlows();
    expect(container.querySelector(".toolbar")).not.toBeNull();
    expect(container.querySelector(".blank")).toBeNull();
  });
});
