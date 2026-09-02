// @vitest-environment jsdom
import type { FunctionComponent } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";

/**
 * **THE STATES THAT ARE NOT THE HAPPY ONE.** A dashboard that only works
 * populated is unfinished; here live the rules of the empty canvas, the loading
 * read and the flow that will not load.
 *
 * **NOTHING IS MEASURABLE IN JSDOM**, and a test reading pixels here would be
 * green for having looked at nothing. What is guarded is the **rule** — who
 * observes what, who fires when, who declares what. The pixels are looked at in
 * a real Chrome, and how is written under "the framing".
 */

/* ── the spy on the framing ─────────────────────────────────────────────────
   `fitView` leaves no trace in jsdom: React Flow's box measures zero and the
   canvas does not move anyway. The only way to know somebody asked for it is to
   intercept the instance React Flow hands over with `onInit`. The real
   ReactFlow passes through here: all we add is a marker on the instance. */

const spy = vi.hoisted(() => ({ fits: [] as unknown[], ready: false }));

interface Instance {
  fitView: (options?: unknown) => unknown;
}

vi.mock("@xyflow/react", async (importOriginal) => {
  const real = await importOriginal<typeof import("@xyflow/react")>();
  const { createElement: element } = await import("react");
  function WatchedReactFlow(props: { onInit?: (instance: Instance) => void }) {
    return element(real.ReactFlow as unknown as FunctionComponent<Record<string, unknown>>, {
      ...props,
      onInit: (instance: Instance) => {
        const fit = instance.fitView.bind(instance);
        instance.fitView = (options?: unknown) => {
          spy.fits.push(options ?? {});
          return fit(options);
        };
        props.onInit?.(instance);
        spy.ready = true;
      },
    });
  }
  return { ...real, ReactFlow: WatchedReactFlow };
});

/* ── the disk with no flows ─────────────────────────────────────────────────
   The empty canvas is not a component: it is **a screen**. Rendering
   `BlankCanvas` alone leaves out exactly what sits beside it — the right panel,
   the minimap, the toolbar — and those are the ones that talked about flows on
   a screen that has none. */

/* Outside the shell the flows come from the sample, and `App` reads it on every
   mount. Here the sample can be taken away for one turn, so the whole screen
   becomes measurable without faking an engine. */

const disk = vi.hoisted(() => ({ empty: false, brokenOnly: false }));

vi.mock("./sample", async (importOriginal) => {
  const real = await importOriginal<typeof import("./sample")>();
  return {
    ...real,
    get SAMPLE() {
      if (disk.empty) return [];
      // A disk with files that will not load and none that will: the scene where
      // the column has no flows to list but does have to show why there are
      // none, and the empty card names it.
      if (disk.brokenOnly) return real.SAMPLE.filter((entry) => entry.state === "broken");
      return real.SAMPLE;
    },
  };
});

import App from "./App";
import { BlankCanvas } from "./BlankCanvas";
import {
  belowThreshold,
  contrastPairs,
  parseStylesheet,
  styleTree,
  type ElementStyle,
  type Stylesheet,
} from "./contrast";
import { stepCountLabel } from "./flow";
import { SAMPLE } from "./sample";
import stylesheetSource from "./styles.css?raw";

/* ── the box observer, driven by the test ───────────────────────────────────
   In the other files `ResizeObserver` is an empty shell: enough for React Flow
   to mount. Here more is needed — **the observer is the subject of the rule** —
   so we record who watches what and the test decides when the box becomes
   measurable. */

interface Watcher {
  element: Element;
  announce: (width: number, height: number) => void;
}

const watchers: Watcher[] = [];

class RecordingResizeObserver {
  private readonly heard: (entries: Array<{ target: Element; contentRect: DOMRectReadOnly }>) => void;
  private readonly mine: Watcher[] = [];

  constructor(callback: (entries: Array<{ target: Element; contentRect: DOMRectReadOnly }>) => void) {
    this.heard = callback;
  }

  observe(element: Element) {
    const watcher: Watcher = {
      element,
      announce: (width, height) =>
        this.heard([{ target: element, contentRect: { width, height } as DOMRectReadOnly }]),
    };
    this.mine.push(watcher);
    watchers.push(watcher);
  }

  unobserve(element: Element) {
    for (const watcher of this.mine.filter((candidate) => candidate.element === element)) {
      this.forget(watcher);
    }
  }

  // STOPPING WATCHING MUST REALLY STOP. With a `disconnect` that removes
  // nothing, "it frames only once" would be red on correct code: the fake would
  // keep talking to whoever hung up.
  disconnect() {
    for (const watcher of [...this.mine]) this.forget(watcher);
  }

  private forget(watcher: Watcher) {
    const here = watchers.indexOf(watcher);
    if (here >= 0) watchers.splice(here, 1);
    const there = this.mine.indexOf(watcher);
    if (there >= 0) this.mine.splice(there, 1);
  }
}

/**
 * Tells whoever observes this element that the box measures so much, and
 * returns **how many** heard it. The number is not a bonus: without it,
 * "nothing got framed" would be true even when nobody is watching — precisely
 * the case this test has to reject.
 */
function announce(element: Element, width: number, height: number): number {
  const listening = watchers.filter((watcher) => watcher.element === element);
  for (const watcher of listening) watcher.announce(width, height);
  return listening.length;
}

let sheet: Stylesheet;

beforeAll(() => {
  sheet = parseStylesheet(stylesheetSource);
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RecordingResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

beforeEach(async () => {
  // React Flow hands the instance over **one tick after** mount, from inside a
  // `setTimeout`. We let the previous turn's instance land before resetting, or
  // the next test would believe an instance that is not its own is ready.
  await new Promise((resolve) => setTimeout(resolve, 5));
  watchers.length = 0;
  spy.fits.length = 0;
  spy.ready = false;
  disk.empty = false;
  disk.brokenOnly = false;
});

afterEach(cleanup);

/* ── a shape is seen, or it is not a shape ──────────────────────────────────
   **COUNTING DOM ELEMENTS LETS A HIDDEN SKELETON THROUGH.** The DOM is one
   door; the sheet is the other. `width: 0` on the step plates and the seven
   plates are gone; `display: none` on the gestures and the empty canvas is back
   to the mute statement this file accuses. Either way the count is identical
   and the suite stays green. */

/* `styleTree` already answers: it computes `hidden` with inheritance and keeps
   the winning declarations. What we look at here are **the forms with which a
   rule removes an element without deleting it** — hidden, transparent,
   flattened, reduced to zero — plus the one that deletes it. */

/** `0`, `0px`, `0%`: the forms in which a measure is no measure at all. */
const NO_SIZE = /^0(\.0+)?[a-z%]*$/i;

/**
 * The properties with which a rule reduces an element to nothing. `min-width: 0`
 * is not among them: it is a flexbox idiom and hides nobody — a `min-*` raises a
 * floor, it does not bring anything to zero.
 */
const SIZES = ["width", "height", "max-width", "max-height"];

/** A fixed height is a number with a unit: `auto`, `100%`, `fit-content` are not. */
const FIXED_LENGTH = /^-?\d*\.?\d+(px|rem|em|ch|ex|vh|vw|vmin|vmax|pt|pc|cm|mm|in|q)$/i;

/** The two words with which a box takes away what does not fit inside it. */
const CLIPPING = ["hidden", "clip"];

/**
 * What removes this element **without writing anything on it**: the clipping,
 * the inherited font size, the box that contains it. `font-size` inherits, so
 * the first declaration met on the way up wins; `overflow` and height do not,
 * and must be looked for on every box up to the measured root.
 */
function whatComesFromTheBox(
  element: Element,
  root: Element,
  styles: Map<Element, ElementStyle>,
): string[] {
  const faults: string[] = [];
  let fontSize: string | undefined;
  let box: Element | null = element;
  while (box !== null) {
    const declarations = styles.get(box)?.declarations;
    if (declarations !== undefined) {
      const path = declarations.get("clip-path");
      if (path !== undefined && path !== "none") faults.push(`ritagliato via (clip-path: ${path})`);

      // The old twin of `clip-path`, the screen-reader-only recipe. It bites
      // **only on a positioned box**: declaring it elsewhere removes nobody, and
      // calling that a fault would be a false positive.
      const clip = declarations.get("clip");
      const edges = clip?.match(/^rect\((.*)\)$/i)?.[1].split(/[,\s]+/) ?? [];
      const positioned = ["absolute", "fixed"].includes(declarations.get("position") ?? "");
      if (positioned && edges.length === 4 && edges.every((edge) => NO_SIZE.test(edge))) {
        faults.push(`ritagliato via (clip: ${clip})`);
      }

      fontSize ??= declarations.get("font-size");

      const overflow = `${declarations.get("overflow") ?? ""} ${declarations.get("overflow-y") ?? ""}`;
      const height = declarations.get("height") ?? declarations.get("max-height");
      const cut = overflow.split(/\s+/).some((word) => CLIPPING.includes(word));
      if (cut && height !== undefined && FIXED_LENGTH.test(height)) {
        faults.push(`dentro una scatola alta ${height} che taglia il resto`);
      }
    }
    if (box === root) break;
    box = box.parentElement;
  }
  if (fontSize !== undefined && NO_SIZE.test(fontSize)) faults.push("corpo del testo a zero");
  return faults;
}

/**
 * `scale(0)`, `scaleY(0)`, `scale(1, 0)`: flattened is as invisible as hidden.
 * The same goes for `scale: 0`, which is the same thing **outside** `transform`:
 * a property of its own, with the factors separated by spaces.
 */
function flattened(declarations: Map<string, string>): boolean {
  const factors = declarations.get("scale");
  if (factors !== undefined && factors.split(/\s+/).some((factor) => NO_SIZE.test(factor))) return true;
  const transform = declarations.get("transform");
  if (transform === undefined) return false;
  for (const call of transform.matchAll(/\bscale(?:3d|x|y|z)?\(([^)]*)\)/gi)) {
    if (call[1].split(",").some((factor) => NO_SIZE.test(factor.trim()))) return true;
  }
  return false;
}

/**
 * What stops these elements from being seen. Empty means they all are. A
 * selector that finds too few is the first fault listed: deleting is not a way
 * of being visible, and without it the check would be green on a screen that
 * lost everything.
 */
function whatHidesThem(
  root: Element,
  sheet: Stylesheet,
  wanted: Array<[selector: string, atLeast: number]>,
): string[] {
  const styles = styleTree(root, sheet);
  const faults = new Set<string>();
  for (const [selector, atLeast] of wanted) {
    const found = Array.from(root.querySelectorAll(selector));
    if (found.length < atLeast) {
      faults.add(`${selector}: ${found.length} invece di ${atLeast}`);
      continue;
    }
    for (const element of found) {
      const style = styles.get(element);
      if (style === undefined) {
        faults.add(`${selector}: fuori dall'albero misurato`);
        continue;
      }
      if (style.hidden) faults.add(`${selector}: nascosto`);
      if (style.opacity === 0) faults.add(`${selector}: trasparente`);
      if (flattened(style.declarations)) faults.add(`${selector}: schiacciato`);
      for (const property of SIZES) {
        const value = style.declarations.get(property);
        if (value !== undefined && NO_SIZE.test(value)) faults.add(`${selector}: ${property} a zero`);
      }
      for (const fault of whatComesFromTheBox(element, root, styles)) {
        faults.add(`${selector}: ${fault}`);
      }
    }
  }
  return [...faults];
}

/** The gestures of the empty canvas, and the plates of the loading state. */
const GESTURES: Array<[selector: string, atLeast: number]> = [
  [".blank__gestures", 1],
  [".blank__gestures li", 3],
];

const SKELETON: Array<[selector: string, atLeast: number]> = [
  [".blank__skeleton", 1],
  [".blank__plate", 6],
  [".blank__plate--step", 7],
];

/** The window opens on «Now»: the board sits behind a place to be chosen. */
function goToFlows(): void {
  fireEvent.click(screen.getByRole("button", { name: /^Board/ }));
}

/* ═══ 1. THE FRAMING ════════════════════════════════════════════════════════ */

/**
 * **THE BOARD WAS BORN WITH ITS FRAMING MEASURED AT ZERO.** The window opens on
 * «Now»; the board lives inside `.body[hidden]`; the sheet gives that a
 * `display: none`; React Flow mounts with `fitView` on a **0×0** box. The two
 * remaining `fitView` calls fire on `focusName` or `source`, never on «Flows».
 * The numbers behind that were measured in a real Chrome — in jsdom nothing has
 * a size, so none is reproduced here and none is faked; what is guarded is the
 * rule that produces them.
 *
 * **THE REMAINDER IS A DECLARED LIMIT, NOT UNFINISHED WORK.** React Flow's
 * default `minZoom` is **0.5** while a framing that held everything would want
 * **0.338**, the canvas being 920px against a `relay` lane of 1040px: the fit
 * asks below the minimum, gets the minimum, and the rest leaves the box. Leave
 * `minZoom` alone — seeing every node costs 25px of window, seeing them whole
 * 264px, and lowering it would change the zoom gesture on every screen for a
 * strip under an inch. The mitigation is **the minimap**, in each state's colour.
 *
 * **THE TEST BELOW IS STRICTER THAN THE WORLD.** Removing the `width === 0`
 * guard turns it red, yet in Chrome nothing would show: there the first
 * `ResizeObserver` notification already arrives with a measured box. The guard
 * forbids framing a null box by construction instead of by luck. **And the
 * fault is not one of framing**: an empty screen with a confident bar beside it
 * explains that emptiness plausibly and falsely, so the broken state and the
 * empty state become indistinguishable — the worst fault in this section.
 */
describe("the framing, when the board appears", () => {
  test("PRESSING «Flows» REDOES THE FRAMING, as soon as the box can be measured", async () => {
    const { container } = render(<App />);
    await vi.waitUntil(() => spy.ready);

    // THE CONDITION IS A HIDDEN BOARD, AND THE WINDOW NOW OPENS ON IT: leaving
    // and coming back is what puts the box back at zero. The fault this guards
    // — framing a box that measures nothing — is unchanged.
    const body = container.querySelector(".body") as HTMLElement;
    fireEvent.click(screen.getByRole("button", { name: /^Memory/ }));
    expect(body.hasAttribute("hidden"), "leaving did not hide the board").toBe(true);

    spy.fits.length = 0;
    goToFlows();
    expect(body.hasAttribute("hidden"), "pressing «Flows» does not show the board").toBe(false);

    const canvas = container.querySelector(".canvas") as HTMLElement;

    // First direction: while the box measures zero, nothing gets framed. Framing
    // a null box is exactly the fault we come from.
    expect(
      announce(canvas, 0, 0),
      "nobody is watching the canvas box: the reframing cannot fire",
    ).toBeGreaterThan(0);
    expect(spy.fits, "a null box got framed").toHaveLength(0);

    // Second direction: as soon as the box has a size, the view is redone.
    announce(canvas, 1200, 800);
    expect(
      spy.fits.length,
      "the box became measurable and nobody redid the framing",
    ).toBeGreaterThan(0);
  });

  test("and it is not redone on every breath of the box", async () => {
    // Reframing on every measurement would snap the view back to center while
    // somebody resizes the window or opens a panel: the gesture is "show the
    // board", not "be wide".
    const { container } = render(<App />);
    await vi.waitUntil(() => spy.ready);
    goToFlows();
    const canvas = container.querySelector(".canvas") as HTMLElement;

    announce(canvas, 1200, 800);
    const first = spy.fits.length;
    // Without this line "it adds none" would be true of a view that never frames
    // at all: the case the whole fault comes from.
    expect(first, "the first measurement framed nothing").toBeGreaterThan(0);

    announce(canvas, 900, 700);
    expect(spy.fits.length, "every width change recenters the canvas").toBe(first);
  });
});

/* ═══ 2. THE PLURAL ═════════════════════════════════════════════════════════ */

/**
 * **A PERSON READS «1 passi».** The number and the plural must be born on the
 * same line, or the column and the lane header drift apart again.
 */
describe("a single step is not «1 passi»", () => {
  test("THE COLUMN AND THE LANE COUNT IN ITALIAN", () => {
    const { container } = render(<App />);
    goToFlows();

    const read = (selector: string) =>
      Array.from(container.querySelectorAll(selector)).map((node) => (node.textContent ?? "").trim());

    // `prima-corsa` is the sample flow with a single step: without it this test
    // would be green for never having met the case.
    const notes = read(".rail__note");
    expect(notes, "no sample flow has a single step").toContain(stepCountLabel(1));
    expect(notes).not.toContain("1 steps");

    const counts = read(".flow-band__count");
    expect(counts, "the lane does not count its steps").toContain(stepCountLabel(1));
    expect(counts).not.toContain("1 steps");

    // And the plural stays plural: "no «1 steps»" would be true of a line that
    // always writes «step» too.
    expect(notes).toContain(stepCountLabel(7));
  });

  test("the line that counts knows how to count", () => {
    expect(stepCountLabel(0)).toBe("0 steps");
    expect(stepCountLabel(1)).toBe("1 step");
    expect(stepCountLabel(7)).toBe("7 steps");
  });
});

/* ═══ 3. THE INVITATIONS ════════════════════════════════════════════════════ */

/**
 * **TWO INVITATIONS ON THE SAME SCREEN CANCEL EACH OTHER** — the rule the
 * toolbar invokes to disappear when there are no flows. So the count is of
 * **who** invites, not of words, and the list is not by hand: it once left out
 * the column, which invites with «+ New flow», and the rule stayed green.
 */
const OWNERS: Array<[selector: string, name: string]> = [
  [".rail", "the column"],
  [".blank__card", "the blank canvas"],
  [".toolbar__prompt", "the bar"],
  [".panel__empty", "the panel"],
];

/**
 * An invitation asks for a gesture **on a flow**: with a verb, or with the sign
 * that stands in for one. Demanding the verb left out the most inviting button
 * on the screen, «+ New flow», which states the gesture with a `+`.
 */
function invites(text: string): boolean {
  return /\bflows?\b/i.test(text) && /\+|\b(pick|choose|create|make|new)\b/i.test(text);
}

/**
 * **THE RULE IS ABOUT ONE GESTURE, NOT ABOUT MENTIONING A FLOW.** The bar's
 * prompt and the column's button both name a flow and both carry a verb, so
 * counting them together said «two invitations» about two different requests.
 * What cancels itself out is the *same* gesture offered twice.
 */
function invitesToCreate(text: string): boolean {
  return /(\+\s*(a\s+)?new|\b(create|make)\b)[^.]{0,30}\bflows?\b|\bflows?\b[^.]{0,20}\bnew\b/i.test(text);
}

function whoInvitesToCreate(container: HTMLElement): string[] {
  const found: string[] = [];
  for (const [selector, name] of OWNERS) {
    const owner = container.querySelector(selector);
    if (owner && invitesToCreate(owner.textContent ?? "")) found.push(name);
  }
  return found;
}

function whoInvites(container: HTMLElement): string[] {
  const found: string[] = [];
  for (const [selector, name] of OWNERS) {
    const owner = container.querySelector(selector);
    if (owner && invites(owner.textContent ?? "")) found.push(name);
  }
  return found;
}

describe("two invitations on the same screen cancel each other", () => {
  /**
   * **THE COLUMN OWNS THE GESTURE, IN EVERY STATE WHERE IT EXISTS.** The bar is
   * scoped to one flow, so a gesture that makes a sibling of it does not belong
   * there, and with a flow focused the bar does not carry it at all. The tools
   * moved into the board because a step is a thing **on** it — a flow is not.
   */
  test("AT REST ONLY THE COLUMN OFFERS TO MAKE A FLOW, and the bar points at it", () => {
    const { container } = render(<App />);
    goToFlows();

    // The scene is the right one: there are flows, none is focused, the toolbar
    // has changed job. Without this line the count would be true even on a
    // screen where nobody speaks.
    const prompt = container.querySelector(".toolbar__prompt") as HTMLElement;
    expect(prompt, "the toolbar has not changed job").not.toBeNull();
    expect(whoInvitesToCreate(container)).toEqual(["the column"]);

    // And the bar still speaks: silencing it would satisfy the count above
    // while leaving the screen with a bar that says nothing at all.
    expect(prompt.textContent ?? "").toMatch(/in the column/i);
    expect(invites(prompt.textContent ?? ""), "the bar stopped asking for a flow").toBe(true);
  });

  test("WITH A FLOW FOCUSED, THE COLUMN IS STILL THE ONLY ONE THAT OFFERS IT", () => {
    const { container } = render(<App />);
    goToFlows();
    fireEvent.click(container.querySelector("button.rail__item") as HTMLElement);

    // The bar has its tools back, so this is the other state, not a repeat of
    // the one above: there the gesture could hide in a bar that had no tools.
    expect(container.querySelectorAll(".toolbar__tool").length, "the bar has no tools").toBeGreaterThan(0);
    expect(whoInvitesToCreate(container)).toEqual(["the column"]);
  });

  test("WITH A FLOW FOCUSED, THE PANEL DOES NOT INVITE FOCUSING IT", () => {
    const { container } = render(<App />);
    goToFlows();
    fireEvent.click(container.querySelector("button.rail__item") as HTMLElement);

    // The flow really is focused, and its name is written at the top: that is
    // what makes the invitation a request to do what is already done.
    const focused = container.querySelector(".focusbar__name, .focusbar__name-input");
    expect(focused, "no flow is focused").not.toBeNull();

    const panel = container.querySelector(".panel__empty") as HTMLElement;
    expect(panel, "the panel says nothing").not.toBeNull();
    expect(
      panel.textContent ?? "",
      "the panel invites focusing a flow that is already focused",
    ).not.toMatch(/\bflows?\b/i);

    // And its job remains: the step, which is what the panel shows.
    expect(panel.textContent ?? "").toMatch(/\bstep\b/i);
  });

  test("ON THE EMPTY CANVAS THE CANVAS INVITES, not the column too", () => {
    // **INSIDE `App`, not the component alone.** Rendered on its own,
    // `BlankCanvas` has no column, toolbar or panel beside it: the three that
    // could invite along with it are missing, and "only one" is true by absence.
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank[data-state='empty']"), "this is not the empty canvas").not.toBeNull();
    expect(whoInvites(container)).toEqual(["the blank canvas"]);
    // The state where the column is closed is the one state where the canvas
    // owns the gesture instead: still one owner, a different one.
    expect(whoInvitesToCreate(container)).toEqual(["the blank canvas"]);
  });
});

/* ═══ 2 bis. A LINE THAT POINTS AT A PLACE YOU ARE NOT IN ═══════════════════ */

/**
 * **THE BAR SENDS YOU TO A COLUMN SIX PLACES OUT OF SEVEN HAVE NOT GOT.** The
 * line it carries with nothing focused — «pick one in the rail» — belongs to
 * the board alone, and the window opens away from the board: two invitations
 * cancelling each other, in its loneliest form.
 */
describe("the bar does not send you to a column this place has not got", () => {
  test("AWAY FROM THE BOARD THE BAR IS SILENT ABOUT THE COLUMN, and speaks on the board", () => {
    const { container } = render(<App />);

    // The window opens on the board now, where the column is: the line must
    // be there. This is the control that makes the absence below worth
    // anything, because an absence passes just as well when the selector is
    // wrong.
    const line = container.querySelector(".topbar__none");
    expect(line, "the bar is silent on the board, where the column is").not.toBeNull();
    expect(line?.textContent ?? "").toMatch(/rail|column/i);

    // Leave the board: **mounted is not in view**. The board sits inside
    // `.body[hidden]`, and the bar must stop naming a column this place has
    // not got.
    fireEvent.click(screen.getByRole("button", { name: /^Memory/ }));
    expect(container.querySelector(".body[hidden]"), "the board is still in view").not.toBeNull();
    expect(
      container.querySelector(".topbar__none"),
      "the bar names the column in a place that has none",
    ).toBeNull();
  });
});


/* ═══ 3 bis. THE SCREEN WITH NO FLOWS, IN FULL ══════════════════════════════ */

/**
 * **A PROMISE ABOUT SOMETHING THAT CANNOT HAPPEN THERE.** With zero flows the
 * right panel said a step's parameters appear here: there are no steps and no
 * flows. The minimap was worse — nothing interrogated it in **either**
 * direction, so it was itself a thing that is seen and says nothing.
 */
describe("the screen with no flows, in full", () => {
  test("THE PANEL IS SILENT AT ZERO FLOWS, and speaks as soon as there is one", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelectorAll("button.rail__item"), "the column still has flows").toHaveLength(0);
    expect(
      container.querySelector(".panel__empty"),
      "the panel promises a step's parameters on a screen with no steps",
    ).toBeNull();

    // The opposite direction: without it, "silent" would be true of a panel that
    // never speaks, and its job would vanish along with the fault.
    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    const panel = populated.container.querySelector(".panel__empty");
    expect(panel, "the panel is silent even where it has something to say").not.toBeNull();
    expect(panel?.textContent ?? "").toMatch(/\bstep\b/i);
  });

  /**
   * **THE THIRD WAY IS NOT BEING THERE.** Silencing the content and leaving the
   * container standing left a 288×818px strip — a fifth of the window — mute and
   * cut off by a hairline: it does not read as calm, it reads as a part of the
   * screen that has not finished loading.
   */
  test("THE RIGHT COLUMN CLOSES AT ZERO FLOWS, and the canvas takes its width", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".panel"),
      "a mute strip beside the screen that teaches the first gesture",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".panel"),
      "the column is gone even where it has a step's parameters to show",
    ).toHaveLength(1);
  });

  /**
   * **THE LEFT COLUMN CLOSES AT ZERO FLOWS TOO.** Not for symmetry: its
   * «+ New flow» and the card's «Create the first flow» are one function
   * under two names, half a metre apart. The gesture keeps its home — the two
   * never coexist: at the first click the flow is born and the column returns.
   */
  test("THE LEFT COLUMN CLOSES AT ZERO FLOWS, and the canvas takes the full width", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".rail"),
      "a list of nothing with its invitation beside it, while the card offers the same gesture",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".rail"),
      "the column is gone even where it has flows to list",
    ).toHaveLength(1);
    expect(
      populated.container.querySelector(".rail__new"),
      "the gesture has lost its permanent home",
    ).not.toBeNull();
  });

  /**
   * **WITH ONLY BROKEN FLOWS THE COLUMN STAYS, AND IS SILENT.** The card says
   * files that will not load sit «at the foot of the column»: closing the column
   * here too would make that line false, and naming a place that is not there is
   * the very fault this whole screen comes from. It shows, it does not call.
   */
  test("WITH ONLY BROKEN FLOWS THE COLUMN STAYS, and only the canvas still invites", () => {
    disk.brokenOnly = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank[data-state='empty']"), "this is not the empty canvas").not.toBeNull();
    expect(
      container.querySelectorAll(".rail__item[data-broken]"),
      "the broken flow vanished along with the column",
    ).toHaveLength(1);

    // The card's line names the column, and the column is there: without this
    // line "the column stays" would be a choice with no reason.
    expect(container.querySelector(".blank__card")?.textContent ?? "").toMatch(/foot of the column/i);
    expect(whoInvites(container)).toEqual(["the blank canvas"]);
  });

  /**
   * **FOUR BUTTONS THAT FRAME NOTHING.** It is word for word "a panel that is
   * seen and says nothing" — the reason the minimap below disappears — applied
   * to React Flow's controls. There is one criterion.
   */
  test("THE CANVAS CONTROLS VANISH WITH THE FLOWS, and come back with them", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".react-flow__controls"),
      "controls that zoom and frame a canvas with nothing in it",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".react-flow__controls"),
      "the controls are gone from the board too, where they control something",
    ).toHaveLength(1);
  });

  test("THE MINIMAP VANISHES WITH THE FLOWS, and comes back with them", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".react-flow__minimap"),
      "a map of nothing on the screen that teaches the first gesture",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".react-flow__minimap"),
      "the minimap is gone from the board too, where it mitigates the declared limit",
    ).toHaveLength(1);
  });
});

/* ═══ 4. THE THREE STATES ═══════════════════════════════════════════════════ */

describe("the three states of the canvas with no flows", () => {
  test("LOADING IS A SHAPE, not a spinner and not a single sentence", () => {
    const { container } = render(<BlankCanvas state="loading" brokenCount={0} onCreate={() => {}} />);

    // A skeleton says **what is coming**: the lanes and their steps, in the
    // place where they will appear. Below six plates it is no longer a shape.
    expect(
      container.querySelectorAll(".blank__plate").length,
      "the loading state has no shape",
    ).toBeGreaterThanOrEqual(6);

    // And the shape is seen. Counting DOM elements is exactly how this test
    // would stay green on a hidden skeleton — and the attribute is one door: the
    // sheet is the other, and from there `width: 0` on the step plates leaves
    // two identical, empty lanes.
    const skeleton = container.querySelector(".blank__skeleton") as HTMLElement;
    expect(skeleton.hidden, "the skeleton is there but nobody sees it").toBe(false);
    expect(
      whatHidesThem(document.documentElement, sheet, SKELETON),
      "the skeleton is in the DOM and the sheet removes it",
    ).toEqual([]);

    // And the word stays: prohibition 5 allows no state readable only from the
    // shape, just as it allows none readable only from the color.
    expect(container.textContent ?? "").toMatch(/engine/i);

    // A skeleton does not invite: there is nothing to decide yet.
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  test("THE EMPTY CANVAS TEACHES THE FIRST GESTURE, and names a place that exists", () => {
    let created = 0;
    const { container } = render(
      <BlankCanvas state="empty" brokenCount={0} onCreate={() => (created += 1)} />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Create the first flow/ }));
    expect(created, "the offered gesture does nothing").toBe(1);

    // The gestures are in a row, and they are the real ones. The toolbox is a bar
    // at the bottom of the canvas, not on the left, and with zero flows it is not
    // there at all — a false instruction is worse than no instruction.
    expect(container.textContent ?? "").not.toMatch(/toolbox on the left|column on the left/i);
    expect(container.querySelectorAll(".blank__gestures li").length).toBeGreaterThanOrEqual(3);

    // And the gestures are seen: `display: none` on their row takes the canvas
    // back to the mute statement this file accuses, without touching the count.
    expect(
      whatHidesThem(document.documentElement, sheet, GESTURES),
      "the gestures are in the DOM and the sheet removes them",
    ).toEqual([]);

    // The sheet draws the numbers on a list with no bullets: without the declared
    // role, whoever reads with their ears loses "a list of three".
    expect(
      container.querySelector(".blank__gestures")?.getAttribute("role"),
      "the gesture list does not announce itself as a list",
    ).toBe("list");
  });

  /**
   * **WHOEVER MEASURES MUST BE MEASURED**, and a check promising more than it
   * proves is the very fault this file accuses in the screen. So the title says
   * how many forms it sees, and the ones it does not see are named here.
   *
   * **THE TEN IT SEES**: deleted, `display: none`, `visibility: hidden`,
   * `opacity: 0`, a scale at zero, a size at zero, `clip-path`,
   * `clip: rect(0,0,0,0)` on a positioned box, a font size of zero, and the box
   * that clips at a fixed height. A scale counts once and holds twice, because
   * `transform: scale(0)` and `scale: 0` are the same thing written in two
   * places — the second a property of its own — and in Chromium the three `li`
   * collapse to 0×0 either way: looking only inside `transform` was an open
   * door. The one-pixel height with `overflow: hidden` is not a case of its
   * own; it falls under the clipping box, and the test below shows it.
   *
   * **EIGHT OF THE ONES IT DOES NOT SEE**: `content-visibility: hidden`;
   * `position: absolute` with an off-screen `left`; `z-index: -1` under an
   * opaque sibling; `color: transparent`; `text-indent: -9999px`;
   * `filter: opacity(0)`; `translateX(-9999px)`; `overflow` with a % height.
   * Eight and not all, a sample: what they lack is one thing, the sheet's
   * computation carried to the end — where an element lands, how big a
   * percentage makes it, who covers whom. In jsdom none of that exists, so the
   * place to win this is a real Chrome and not one more rule here.
   */
  test("THE SHAPE CHECK SEES TEN WAYS OF VANISHING, and eight it does not see are written above", () => {
    render(<BlankCanvas state="empty" brokenCount={0} onCreate={() => {}} />);

    // With the real sheet it complains about nothing: a check that always
    // complains checks nothing, and the two tests above would be green by luck.
    expect(whatHidesThem(document.documentElement, sheet, GESTURES)).toEqual([]);

    const tricks = [
      ".blank__gestures { display: none }",
      ".blank__gestures { visibility: hidden }",
      ".blank__gestures { opacity: 0 }",
      ".blank__gestures { transform: scale(0) }",
      ".blank__gestures { scale: 0 }",
      ".blank__gestures { position: absolute; clip: rect(0, 0, 0, 0) }",
      ".blank__gestures { max-height: 0 }",
      ".blank__gestures li { height: 0 }",
      ".blank__gestures li { transform: scale(1, 0) }",
      ".blank__gestures { clip-path: inset(100%) }",
      ".blank__gestures li { font-size: 0 }",
      ".blank__card { overflow: hidden; height: 150px }",
      ".blank__gestures li { height: 1px; overflow: hidden }",
    ];
    for (const trick of tricks) {
      const doctored = parseStylesheet(`${stylesheetSource}\n${trick}`);
      expect(whatHidesThem(document.documentElement, doctored, GESTURES), trick).not.toEqual([]);
    }

    // And the eight left out really do stay out: if one went red, the list above
    // would be lying in the other direction.
    const blind = [
      ".blank__gestures { content-visibility: hidden }",
      ".blank__gestures { position: absolute; left: -9999px }",
      ".blank__gestures { z-index: -1 }",
      ".blank__gestures { color: transparent }",
      ".blank__gestures li { text-indent: -9999px }",
      ".blank__gestures { filter: opacity(0) }",
      ".blank__gestures { transform: translateX(-9999px) }",
      ".blank__card { overflow: hidden; height: 10% }",
    ];
    for (const spot of blind) {
      const doctored = parseStylesheet(`${stylesheetSource}\n${spot}`);
      expect(whatHidesThem(document.documentElement, doctored, GESTURES), spot).toEqual([]);
    }

    // And deleting is not a way of being visible: on a screen that has no
    // gestures at all the sheet is innocent, and the check says so anyway.
    cleanup();
    render(<BlankCanvas state="loading" brokenCount={0} onCreate={() => {}} />);
    expect(whatHidesThem(document.documentElement, sheet, GESTURES)).not.toEqual([]);
  });

  test("A FLOW THAT WILL NOT LOAD CARRIES THE REASON, word for word", () => {
    // A promise already kept, but nothing interrogated it: summarising the
    // reason, or dropping it, left the suite green.
    const broken = SAMPLE.find((entry) => entry.state === "broken");
    expect(broken, "the sample data no longer has a broken flow").toBeDefined();
    const reason = broken?.state === "broken" ? broken.broken.reason : "";

    const { container } = render(<App />);
    goToFlows();

    const marked = container.querySelector(".rail__item[data-broken]") as HTMLElement;
    expect(marked, "the broken flow vanished from the column instead of staying marked").not.toBeNull();
    expect(marked.textContent ?? "").toContain(reason);
  });

  /**
   * **TWO NEW SCREENS, TWO NEW SCENES TO MEASURE.** Prohibition 6 lives in
   * `contrast.test.tsx`, and its three scenes are all populated: the empty
   * canvas and the loading state crossed none of them. Drawing them without
   * measuring them would reopen the hole that file was born to close.
   */
  test("THE TWO NEW SCREENS BRING NO PAIR BELOW 4.5:1", () => {
    // Each scene declares how many pairs it expects to find: whoever measures
    // must be measured, and a scene that finds nothing would pass for the wrong
    // reason. The loading state has few on purpose — it is a shape, and its only
    // word sits at the bottom.
    for (const [state, atLeast] of [["empty", 6], ["loading", 3]] as const) {
      cleanup();
      render(<BlankCanvas state={state} brokenCount={2} onCreate={() => {}} />);
      const pairs = contrastPairs(document.documentElement, sheet);
      expect(pairs.length, `«${state}» has not enough text to measure`).toBeGreaterThanOrEqual(atLeast);
      expect(belowThreshold(pairs), `pairs below threshold in «${state}»`).toEqual([]);
    }
  });

  test("and a mute engine offers no gesture it could not honour", () => {
    // A flow created while the engine is silent could not be saved: offering it
    // would be a promise nobody can keep.
    const { container } = render(
      <BlankCanvas state="failed" failure="il motore non risponde" brokenCount={0} onCreate={() => {}} />,
    );
    expect(container.querySelectorAll("button")).toHaveLength(0);
    expect(container.textContent ?? "").toContain("il motore non risponde");
  });
});
