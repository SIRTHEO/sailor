// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";

import App from "./App";
import stylesheetSource from "./styles.css?raw";
import reactFlowSource from "@xyflow/react/dist/style.css?raw";
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
 * **THE STEP TOOLBOX, INTERROGATED WHERE IT WOULD GO WRONG.** A screenshot
 * cannot show that the bar sits **inside** the canvas without scrolling away,
 * that every family offered creates a step with an action the engine knows in
 * both directions, or that with no flow in focus it says what is missing.
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

/* ── the corridor, read from the stylesheet ─────────────────────────────── */

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
 * The declared lengths: the `:root` grid and the `.toolbar` corridor. They are
 * the only ones the arithmetic below can read. What makes a hand-written
 * literal inside a `calc` turn the test red is the empty remainder demanded at
 * the end of `sumOfTerms`, not this list.
 */
const LENGTHS = new Map<string, number>();
for (const selector of [":root", ".toolbar"]) {
  for (const [property, value] of declarationsOf(selector)) {
    const pixels = /^(\d+(?:\.\d+)?)px$/.exec(value);
    if (property.startsWith("--") && pixels) LENGTHS.set(property, Number(pixels[1]));
  }
}

function lengthOf(name: string): number {
  const value = LENGTHS.get(name);
  expect(value, `«${name}» is not a length declared in the stylesheet`).toBeDefined();
  return value as number;
}

/**
 * The signed sum of a `calc`'s terms, the only form the corridor can be read
 * back in. It returns the **fixed pixels**; the canvas width has its own
 * accumulator, and `canvasTimes` says how often it is due. `100%` is counted
 * with sign and coefficient, never stripped: the rule rests on it cancelling.
 */
function sumOfTerms(value: string, canvasTimes: number): number {
  const body = /^calc\((.*)\)$/.exec(value.trim())?.[1] ?? value.trim();
  const term = /(^|[+-])\s*(?:(\d+)\s*\*\s*)?(?:var\((--[a-z0-9-]+)\)|(\d+)%)/g;
  let total = 0;
  let canvas = 0;
  let read = 0;
  let match: RegExpExecArray | null;
  while ((match = term.exec(body)) !== null) {
    const sign = match[1] === "-" ? -1 : 1;
    const times = match[2] === undefined ? 1 : Number(match[2]);
    if (match[3] !== undefined) total += sign * times * lengthOf(match[3]);
    else canvas += (sign * times * Number(match[4])) / 100;
    read += 1;
  }
  // A term the arithmetic cannot read would vanish silently and the total would
  // come out smaller than the truth, which is the reassuring direction.
  const written = (body.match(/var\(|%/g) ?? []).length;
  expect(read, `the arithmetic cannot read «${value}»`).toBe(written);

  // AND COUNTING WORDS IS NOT ENOUGH: those are occurrences, not terms. A `px`
  // literal next to a `var()` keeps the count of `var(` right and still drops
  // out of the total, and on `margin-left` that loosens instead of tightening.

  // So the right direction is asked of the REMAINDER: once the terms we can
  // read are consumed, only the separators may be left in the body. Anything
  // else left over is a piece nobody measured, and it is refused, not guessed.
  const rest = body.replace(term, "").replace(/[+\-*\s]/g, "");
  expect(
    rest,
    `«${value}» carries «${rest}», which nobody here can measure: the total would skip it silently`,
  ).toBe("");

  // AND THE CANVAS IS COUNTED, not stripped. The arithmetic below is free of
  // the canvas width only if `100%` appears exactly as many times as expected:
  // once in the ceiling, cancelling against the `100%` the minimap band starts
  // from, and never in the offset, which is a fixed distance from the side.
  expect(
    canvas,
    `«${value}» carries the canvas width ${canvas} times instead of ${canvasTimes}: ` +
      `the «100%» no longer cancels out, and the arithmetic below would hold at one width only`,
  ).toBe(canvasTimes);
  return total;
}

/**
 * **THE BAR DECLARES THE CORRIDOR IT DOES NOT OCCUPY.** Centring a bar as wide
 * as its tools works at one width only, and in jsdom nothing has dimensions,
 * so no pixels are measured here: the **rule** is read instead — where the bar
 * starts, and how much of the frame its ceiling grants it.
 */
describe("the corridor the bar does not occupy", () => {
  test("THE BAR IS MEASURED ON THE CORRIDOR, NOT ON THE SUM OF ITS TOOLS", () => {
    const toolbar = declarationsOf(".toolbar");
    const controls = lengthOf("--controls-reserve");
    const minimap = lengthOf("--minimap-reserve");

    // Where the left edge starts, measured from the side of the canvas: a fixed
    // distance, hence zero times the canvas width.
    const offset = toolbar.get("margin-left");
    expect(offset, "the bar declares no offset from the side of the canvas").toBeDefined();
    const left = sumOfTerms(offset as string, 0);

    // What the ceiling grants it: the canvas **exactly once**, minus everything
    // the `calc` takes away.
    const ceiling = toolbar.get("max-width");
    expect(ceiling, "the bar declares no width ceiling").toBeDefined();
    expect(
      (ceiling as string).replace(/\s+/g, " "),
      "the bar's ceiling does not start from the canvas width",
    ).toMatch(/^calc\(100% -/);
    const kept = -sumOfTerms(ceiling as string, 1);

    // THE RULE. Worst right edge = 100% - kept + left. The minimap starts at
    // 100% - minimap. The `100%` cancels on both sides — and that there is
    // exactly one per side has just been checked by `sumOfTerms`, with the
    // coefficient 1 on the ceiling and 0 on the offset.
    const reach = kept - left;
    const where =
      reach >= 0 ? `${reach}px from the right edge` : `${-reach}px PAST the right edge`;
    expect(
      left + minimap,
      `the bar can enter the minimap band: it starts ${left}px from the left edge ` +
        `and its ceiling lets it reach ${where} of the canvas, while the minimap takes ${minimap}px`,
    ).toBeLessThanOrEqual(kept);

    // And from the other side, that the zoom controls stay uncovered.
    expect(
      left,
      `the bar starts at ${left}px and the zoom controls reach ${controls}px`,
    ).toBeGreaterThanOrEqual(controls);
  });

  test("the offset only counts if the panel is anchored to a side", () => {
    // `margin-left` on a `bottom-center` panel takes the bar out of no band at
    // all: React Flow centres it with a `translateX`, and the declared offset
    // would merely push it off centre. The rule above would then be true on the
    // stylesheet and false on screen.
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);
    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar.classList.contains("left")).toBe(true);
    expect(toolbar.classList.contains("center")).toBe(false);
  });

  test("THE RESERVES ARE NOT INVENTED NUMBERS: React Flow dictates them", () => {
    // The arithmetic above would happily use two reserves that are too small —
    // they are declared in the same stylesheet that checks them, and two copies
    // that err together confirm each other. The anchor sits outside both: the
    // real tenants of the bottom band, read where they can be read without
    // layout.
    const theirs = parseStylesheet(reactFlowSource);
    const declaration = (selector: string, property: string) => {
      const rule = theirs.rules.find((candidate) => candidate.selector === selector);
      expect(rule, `React Flow no longer has a «${selector}» rule`).toBeDefined();
      const value = new Map(rule!.declarations).get(property);
      expect(value, `«${selector}» no longer declares «${property}»`).toBeDefined();
      return Number(/^(\d+(?:\.\d+)?)px$/.exec(value as string)?.[1]);
    };

    // The margin React Flow uses to keep EVERY panel off the side: it is what
    // holds the controls and the minimap away from the edge, and it is also the
    // one the bar rewrites for itself.
    const panelMargin = declaration(".react-flow__panel", "margin");
    const buttonWidth = declaration(".react-flow__controls-button", "width");

    // The minimap has no width in the stylesheet: it comes from the `svg` that
    // draws it, and an attribute exists even without layout.
    const { container } = render(<App />);
    goToFlows();
    const minimap = container.querySelector(".react-flow__minimap svg") as SVGElement;
    expect(minimap, "the minimap is not drawn").not.toBeNull();
    const minimapWidth = Number(minimap.getAttribute("width"));
    expect(Number.isFinite(minimapWidth) && minimapWidth > 0).toBe(true);

    expect(
      lengthOf("--controls-reserve"),
      `the zoom controls take ${panelMargin + buttonWidth}px from the side`,
    ).toBeGreaterThanOrEqual(panelMargin + buttonWidth);
    expect(
      lengthOf("--minimap-reserve"),
      `the minimap takes ${panelMargin + minimapWidth}px from the side`,
    ).toBeGreaterThanOrEqual(panelMargin + minimapWidth);
  });

  test("the tools wrap instead of bursting the corridor", () => {
    // A width ceiling on a flex container does not hold its children: the box
    // shrinks to the ceiling and the content spills out anyway, over the
    // minimap, while the bar's `getBoundingClientRect` reports an obedient
    // narrow box. Without these lines the rule would be true only on paper.

    // AND THERE ARE TWO, not one. The row's `flex-wrap` saves the case where
    // three groups do not fit side by side; the group's saves the case where a
    // whole group does not fit, and the corridor drops below the width of a
    // three-tool group long before it disappears. Interrogating one selector
    // only would let the other through.
    for (const selector of [".toolbar__row", ".toolbar__group"]) {
      expect(
        declarationsOf(selector).get("flex-wrap"),
        `«${selector}» does not wrap: a corridor narrower than its content spills over the minimap`,
      ).toBe("wrap");
    }
  });
});

describe("where the bar is", () => {
  test("IT SITS INSIDE THE CANVAS, AND DOES NOT SCROLL AWAY WITH IT", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar).not.toBeNull();

    // Inside the canvas: it is no longer a piece of the rail next to it.
    expect(toolbar.closest(".react-flow")).not.toBeNull();
    expect(toolbar.closest(".rail")).toBeNull();

    // And it does not scroll away: `.react-flow__viewport` is the element that
    // carries the pan and zoom `transform`. A bar inside that would drift off on
    // the first drag, and a screenshot of a still canvas would never say so.
    expect(toolbar.closest(".react-flow__viewport")).toBeNull();
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

  test("the rail no longer holds any tool", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);
    const rail = container.querySelector(".rail") as HTMLElement;
    expect(rail.querySelectorAll(".toolbar__tool")).toHaveLength(0);
    expect(rail.querySelectorAll(".palette__item")).toHaveLength(0);
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

  test("the bar says which flow the step lands in", () => {
    const { container } = render(
      <Toolbar flowName="esamina-la-repo" onAdd={() => {}} />,
    );
    expect(container.querySelector(".toolbar__target")?.textContent).toContain("esamina-la-repo");
  });
});

describe("with no flow picked", () => {
  test("THE BAR SAYS WHAT IS MISSING, IT DOES NOT JUST GO DARK", () => {
    const { container } = render(<Toolbar flowName={null} onAdd={() => {}} />);

    // No disabled button: a button that cannot be pressed and does not say why
    // is a dead end.
    expect(container.querySelectorAll("button:disabled")).toHaveLength(0);
    expect(container.querySelectorAll(".toolbar__tool")).toHaveLength(0);

    // In their place, what is missing — on screen, not inside a `title`.
    const prompt = container.querySelector(".toolbar__prompt") as HTMLElement;
    expect(prompt).not.toBeNull();
    expect(prompt.textContent).toContain("Pick a flow");

    // And it says where, because a bar that names a lack without naming its
    // owner sends people looking. It does not carry the gesture itself: that
    // one belongs to the column, in every state, and is asserted there.
    expect(prompt.textContent).toContain("in the column");
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  test("the reason is not in a `title`, where nobody looks for it", () => {
    const { container } = render(<Toolbar flowName={null} onAdd={() => {}} />);
    const withTitle = Array.from(container.querySelectorAll("[title]"));
    expect(withTitle).toEqual([]);
  });
});

/**
 * **WITH ZERO FLOWS THE BAR IS NOT THERE AT ALL**, which is not the same as
 * «no flow in focus»: there the bar stays and changes job. That moment belongs
 * to the empty canvas; two invitations on one screen cancel each other out. The
 * condition is a single line of `App.tsx` (`flows.size > 0`).
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
