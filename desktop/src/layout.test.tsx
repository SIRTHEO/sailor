// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import layoutSource from "./layout.ts?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { FlowBandNode, type FlowBandData } from "./StepNode";
import { parseStylesheet, styleTree, type Stylesheet } from "./contrast";
import { BAND_DESC_LINES, BAND_HEAD_GAP, BAND_PAD_TOP, edgeLook } from "./layout";
import type { StepRun } from "./flow";

/**
 * **A LANE'S HEADER MUST FIT THE SPACE RESERVED FOR IT.**
 *
 * `BAND_PAD_TOP` is the height the layout leaves free above a lane's first
 * node; the header's sizes live in `styles.css`. The two numbers were not
 * talking to each other: when the sizes grew — a flow's name from 13px to 15px,
 * its description from 11px to 15px from far — `BAND_PAD_TOP` stayed 54, and
 * the description ended up **4px inside the first node**.
 *
 * On the **opening view**, no less: with two flows the canvas picks zoom 0.5,
 * which is below `FAR_ZOOM`, so the «from far» mode is not an edge case — it is
 * what you see when the window opens.
 *
 * This test does not copy the sizes: it reads them from the sheet, on the real
 * component, in both modes.
 */

afterEach(cleanup);

let sheet: Stylesheet;

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  sheet = parseStylesheet(stylesheetSource);
});

const BAND: FlowBandData = {
  name: "prima-corsa",
  description:
    "Il flusso più piccolo che esista: una verifica sola, per vedere il motore girare da un capo all'altro.",
  stepCount: 1,
  color: "#2563eb",
  dimmed: false,
};

function mountBand(): HTMLElement {
  const props = {
    id: "band::prima-corsa",
    type: "flowBand",
    data: BAND,
    selected: false,
    zIndex: -1,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
  } as unknown as NodeProps;
  const { container } = render(
    <ReactFlowProvider>
      <FlowBandNode {...props} />
    </ReactFlowProvider>,
  );
  return container.querySelector(".flow-band") as HTMLElement;
}

/** A number in px written in the sheet, or the error naming the missing one. */
function pixels(value: string | undefined, what: string): number {
  const number = Number.parseFloat(String(value));
  expect(`${what}=${String(value)}`).toMatch(/=\d/);
  return number;
}

/** The height of a text line: size times leading, both of them declared. */
function lineHeight(declarations: Map<string, string>, what: string): number {
  const size = pixels(declarations.get("font-size"), `${what} font-size`);
  const factor = Number.parseFloat(String(declarations.get("line-height")));
  // With `normal` the height depends on the face installed on the machine: no
  // test can see it, and the arithmetic below would become a fiction.
  expect(`${what} line-height=${String(declarations.get("line-height"))}`).toMatch(/=\d/);
  return size * factor;
}

/**
 * How tall a lane's header really is, in canvas units: the padding on top, the
 * tallest line of the header, the gap and the two lines of the description.
 */
function headerHeight(far: boolean): number {
  const band = mountBand();
  if (far) band.setAttribute("data-far", "true");
  else band.removeAttribute("data-far");

  const styles = styleTree(document.documentElement, sheet);
  const read = (selector: string) => {
    const element = band.querySelector(selector) ?? band;
    const style = styles.get(element);
    expect(style, `manca lo stile calcolato di ${selector}`).toBeDefined();
    return (style as { declarations: Map<string, string> }).declarations;
  };

  const bandStyle = styles.get(band)?.declarations as Map<string, string>;
  const padTop = pixels(String(bandStyle.get("padding")).split(/\s+/)[0], "padding della corsia");
  const name = lineHeight(read(".flow-band__name"), ".flow-band__name");
  const count = lineHeight(read(".flow-band__count"), ".flow-band__count");
  const desc = read(".flow-band__desc");
  const gap = pixels(desc.get("margin-top"), ".flow-band__desc margin-top");
  const descLine = lineHeight(desc, ".flow-band__desc");

  return padTop + Math.max(name, count) + gap + descLine * BAND_DESC_LINES;
}

describe("lo spazio riservato all'intestazione di una corsia", () => {
  test("DA VICINO ci sta, col respiro dichiarato", () => {
    expect(BAND_PAD_TOP).toBeGreaterThanOrEqual(headerHeight(false) + BAND_HEAD_GAP);
  });

  test("DA LONTANO ci sta — ed è la vista d'apertura, non un caso limite", () => {
    // The fault sat entirely here: from near the header almost fitted, from far
    // it did not, and from far is how the window opens.
    expect(BAND_PAD_TOP).toBeGreaterThanOrEqual(headerHeight(true) + BAND_HEAD_GAP);
  });

  test("da lontano l'intestazione è davvero più alta che da vicino", () => {
    // Without this one, the two tests above would stay green even if the
    // `[data-far]` rules vanished: they would measure the same scene twice.
    expect(headerHeight(true)).toBeGreaterThan(headerHeight(false));
  });
});

/**
 * **THE CORD SAYS WHETHER THE RUN CAME THROUGH IT.**
 *
 * Before this, every cord on the canvas was the same grey, or the tint of its
 * lane — which the band underneath already said. Which path a run actually took
 * was drawn nowhere, on a surface whose whole job is showing that.
 */
describe("what a cord says", () => {
  const went: StepRun = { step_id: "a", state: "went", attempt: 1 };
  const running: StepRun = { step_id: "b", state: "running", attempt: 1 };
  const waiting: StepRun = { step_id: "b", state: "waiting", attempt: 1 };

  test("the path taken is a full green line", () => {
    const look = edgeLook(false, went, went);
    expect(look.stroke).toBe("var(--state-went)");
    expect(look.dash).toBeUndefined();
    expect(look.live).toBe(false);
  });

  test("the cord into a live step is alive, and says so by moving", () => {
    const look = edgeLook(false, went, running);
    expect(look.stroke).toBe("var(--state-running)");
    expect(look.live).toBe(true);
  });

  test("A CORD NOBODY WALKED IS NOT GREEN: it stays the quiet line", () => {
    // The defect this guards is the tempting one — colouring by the source
    // alone. The source went; the target has not started; nothing came through.
    expect(edgeLook(false, went, waiting).stroke).toBe("var(--line)");
    expect(edgeLook(false, went, undefined).stroke).toBe("var(--line)");
  });

  test("a skippable dependency is broken, whether or not it was walked", () => {
    // The dash is what carries "this data may never arrive", so the meaning
    // survives in greyscale (prohibition 5).
    expect(edgeLook(true, went, went).dash).toBe("5 4");
    expect(edgeLook(true, undefined, undefined).dash).toBe("5 4");
  });
});

/**
 * **NO COLOUR WRITTEN BY HAND, AND THIS FILE HAD TWELVE.**
 *
 * `stylesheet.test.ts` reads `styles.css` and nothing else, so the palette that
 * lived in `layout.ts` — a Tailwind ramp taken whole, plus a `#94a3b8` the
 * direction had abolished for making 2,56:1 — was invisible to every check in
 * the tree. The canvas sheet has the same hole, and the same guard closes it.
 */
describe("the canvas holds no literal colour", () => {
  const LITERAL = /#[0-9a-fA-F]{3,8}\b|\brgba?\(|\bhsla?\(/g;

  test("layout.ts names roles, never tints", () => {
    expect(layoutSource.match(LITERAL) ?? []).toEqual([]);
  });

  // The sheet half of this rule now lives in `stylesheet.test.ts`, which reads
  // the whole file rather than one section. This one stays because no other
  // check reads `layout.ts`, where the tints used to hide.
});

/**
 * THE CORD SAYS WHERE THE FLOW HAS ALREADY BEEN, and not by colour alone —
 * prohibition 5. Full ink behind, a light thread ahead: a path taken and a path
 * not yet taken were the same 1.5px, so the only thing telling them apart was a
 * hue, and one reader in twelve does not separate two of these.
 */
describe("the weight of a cord", () => {
  const went: StepRun = { step_id: "a", state: "went", attempt: 1 };
  const waiting: StepRun = { step_id: "b", state: "waiting", attempt: 1 };
  const running: StepRun = { step_id: "b", state: "running", attempt: 1 };

  test("a path already taken is heavier than one not taken", () => {
    expect(edgeLook(false, went, went).width).toBeGreaterThan(edgeLook(false, went, waiting).width);
  });

  test("a live cord is the heaviest, because it is the one thing moving", () => {
    const live = edgeLook(false, went, running);
    expect(live.width).toBeGreaterThanOrEqual(edgeLook(false, went, went).width);
    expect(live.live).toBe(true);
  });

  test("an untaken cord still has a weight, rather than none", () => {
    expect(edgeLook(false, went, waiting).width).toBeGreaterThan(0);
  });
});
