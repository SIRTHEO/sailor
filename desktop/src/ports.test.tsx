// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import {
  SHIPPED_WITH_THE_BINARY,
  parseFlow,
  readRealFlows,
  shippedFlowsMissingFrom,
} from "./realflows";
import type { FlowFile, Graph, Step, StepRun, StepState, ValueSchema } from "./flow";
import {
  COLUMN,
  MIN_NODE_GAP,
  OUTPUT_PORT_NAME,
  ROOT_PORT_NAME,
  STEP_WIDTH,
  portsOf,
} from "./layout";
import {
  StepNode,
  StepRunContext,
  StepUsageContext,
  stepThatCallsForAGesture,
  type StepNodeData,
} from "./StepNode";
import { contrastRatio, parseColor, parseStylesheet, styleTree, type Stylesheet } from "./contrast";

/**
 * **THE PORTS, AND THE PROMISE THEY CARRY.** The draft declares three things,
 * each with a line here that can turn red: the type lives in the **shape** —
 * circle, diamond, square — not in the tint; a port is **empty when unwired,
 * filled when wired**; and both survive **greyscale**, which is prohibition 5.
 */

// The third is measured, not asserted: the backdrop a browser would give a
// wired mark and an unwired one must stay far apart **in luminance**. A design
// telling them apart with two tints of the same lightness would pass the eye
// and fail here.

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

// ── the test world ──────────────────────────────────────────────────────

const ANY: ValueSchema = { type: "any" };

function object(
  properties: Record<string, ValueSchema>,
  required: string[] = [],
): ValueSchema {
  return { type: "object", properties, required, allow_extra: false };
}

function stepOf(over: Partial<Step>): Step {
  return {
    id: "passo",
    deps: [],
    input_schema: ANY,
    output_schema: ANY,
    with: null,
    when: null,
    action: "external_engine",
    max_attempts: 1,
    ...over,
  };
}

function graphOf(steps: Step[], skippable: Graph["skippable_dependencies"] = []): Graph {
  return { steps, skippable_dependencies: skippable };
}

// ── the real world: the flows the engine actually loads ─────────────────

/**
 * The real flow files, read as raw text and decoded here — no TypeScript schema
 * between the file and what the engine would load. Through the bundler and not
 * `node:fs`, which would want `@types/node`, a tenth dependency on a project
 * that keeps nine. Only the tests import them, so they never ship.
 */
function realFlows(): FlowFile[] {
  // The reader lives in `realflows.ts` — one copy, because two copies is how
  // this test and `StepEditor.test.tsx` came to hold two separate counts of the
  // same directory and go red together the day it emptied.
  return readRealFlows().map(({ path, source }) => parseFlow<FlowFile>(path, source));
}

interface PortCensus {
  total: number;
  text: number;
  structure: number;
  value: number;
  wired: number;
  empty: number;
}

/** How many ports, of which shape and how many fed, over a set of flows. */
function portCensus(flows: FlowFile[]): PortCensus {
  const census: PortCensus = { total: 0, text: 0, structure: 0, value: 0, wired: 0, empty: 0 };
  for (const flow of flows) {
    for (const step of flow.graph.steps) {
      const ports = portsOf(flow.graph, step, flow.inputs ?? {});
      for (const port of [...ports.inputs, ports.output]) {
        census.total += 1;
        census[port.shape] += 1;
        if (port.wired) census.wired += 1;
        else census.empty += 1;
      }
    }
  }
  return census;
}

// ── 1. ports are read from the file, never invented ─────────────────────

describe("where a node gets its own ports from", () => {
  test("a property the dependency produces is WIRED", () => {
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" } }, ["piano"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs).toHaveLength(1);
    expect(ports.inputs[0]).toMatchObject({ name: "piano", wired: true, feed: "upstream" });
  });

  test("a REQUIRED property nobody feeds is EMPTY, and says so", () => {
    // It is the question the draft says it wants to close: which input is
    // connected to nothing, without opening the panel.
    const upstream = stepOf({ id: "monte", output_schema: object({ altro: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ repo: { type: "string" } }, ["repo"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ name: "repo", wired: false, required: true });
  });

  test("a value written in `with` feeds the port: it wins over what arrives", () => {
    // It is the engine's rule, not a choice of this window: `overlay_input` puts
    // `with` on top of the composed input.
    const upstream = stepOf({ id: "monte", output_schema: object({}) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ tool: { type: "array", items: { type: "string" } } }, ["tool"]),
      with: { tool: ["claude-code"] },
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ name: "tool", wired: true, feed: "fixed" });
  });

  test("with SEVERAL dependencies the input is keyed per dependency, and so are the ports", () => {
    const a = stepOf({ id: "a" });
    const b = stepOf({ id: "b" });
    const step = stepOf({
      id: "valle",
      deps: ["a", "b"],
      input_schema: object({ a: ANY, b: ANY, assente: ANY }),
    });
    const ports = portsOf(graphOf([a, b, step]), step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired])).toEqual([
      ["a", true],
      ["b", true],
      ["assente", false],
    ]);
  });

  test("A SINGLE DEPENDENCY, BUT SKIPPABLE: the input is keyed, not that dependency's output", () => {
    // **THE ONE SUBTLE RULE THIS WINDOW COPIES FROM THE ENGINE.** `step_input`
    // in `crates/flow/src/executor.rs` writes
    // `[only] if !graph.dependency_is_skippable(...)`: with a single **non**
    // skippable dependency the input *is* its output. If that lone dependency is
    // skippable the guard fails, we fall into the `many` branch, and the input
    // becomes an object with **one key per dependency**.

    // One dependency is the only case where that condition decides anything:
    // with two, the branch is already the right one for another reason.
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" }, monte: ANY }),
    });
    const graph = graphOf([upstream, step], [{ step: "valle", dependency: "monte" }]);
    const ports = portsOf(graph, step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired, port.feed])).toEqual([
      // `piano` does NOT arrive: it sits inside `monte`, not next to it.
      ["piano", false, "none"],
      // `monte` does: it is the key the `many` branch writes.
      ["monte", true, "upstream"],
    ]);
  });

  test("the same lone dependency, NOT skippable, opens its own properties instead", () => {
    // The twin of the test above: without it the pair does not say that
    // skippability is what decides, only what one case looks like.
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" }, monte: ANY }),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired, port.feed])).toEqual([
      ["piano", true, "upstream"],
      ["monte", false, "none"],
    ]);
  });

  test("when the dependency declares `any` NOBODY IS BLAMED: it is «non lo so»", () => {
    // Three states and not two, as the tool's own panel already does: saying
    // «manca» about an input that may well arrive is an invented accusation.
    const upstream = stepOf({ id: "monte", output_schema: ANY });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ qualcosa: { type: "string" } }, ["qualcosa"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ wired: true, feed: "unknown" });
  });

  test("a step with no dependencies shows the START port, filled only if the file opens it", () => {
    const step = stepOf({ id: "radice" });
    const graph = graphOf([step]);
    expect(portsOf(graph, step, {}).inputs[0]).toMatchObject({
      name: ROOT_PORT_NAME,
      wired: false,
    });
    expect(portsOf(graph, step, { radice: { mandato: "x" } }).inputs[0]).toMatchObject({
      name: ROOT_PORT_NAME,
      wired: true,
    });
  });

  test("THE OUTPUT IS EMPTY WHEN NOBODY READS IT, and that is what the real flows show", () => {
    // On the real flows almost every input is filled by `with`:
    // if filled/empty lived only there, the promise would be green and mute.
    // Leaves, on the other hand, exist in every flow and show up at once.
    const leaf = stepOf({ id: "foglia", deps: ["monte"] });
    const upstream = stepOf({ id: "monte" });
    const graph = graphOf([upstream, leaf]);
    expect(portsOf(graph, leaf, {}).output).toMatchObject({
      name: OUTPUT_PORT_NAME,
      wired: false,
    });
    expect(portsOf(graph, upstream, {}).output).toMatchObject({ wired: true });
  });

  test("with no input schema the ports are the dependencies, and a skippable one is not required", () => {
    const a = stepOf({ id: "a", output_schema: { type: "string" } });
    const b = stepOf({ id: "b" });
    const step = stepOf({ id: "valle", deps: ["a", "b"] });
    const graph = graphOf([a, b, step], [{ step: "valle", dependency: "b" }]);
    const ports = portsOf(graph, step, {});
    expect(ports.inputs).toEqual([
      { name: "a", shape: "text", wired: true, required: true, feed: "upstream" },
      { name: "b", shape: "value", wired: true, required: false, feed: "upstream" },
    ]);
  });

  test("type becomes shape: text a circle, structure a diamond, the rest a square", () => {
    const step = stepOf({
      id: "forme",
      input_schema: object({
        parola: { type: "string" },
        oggetto: object({}),
        elenco: { type: "array", items: ANY },
        numero: { type: "number" },
      }),
    });
    const shapes = portsOf(graphOf([step]), step, {}).inputs.map((port) => port.shape);
    expect(shapes).toEqual(["text", "structure", "structure", "value"]);
  });
});

// ── 2. the node as drawn ────────────────────────────────────────────────

const WIRED_AND_EMPTY: Step = stepOf({
  id: "implementa",
  deps: ["piano"],
  input_schema: object({ piano: { type: "string" }, repo: { type: "string" } }, ["repo"]),
  output_schema: { type: "string" },
});

const UPSTREAM: Step = stepOf({
  id: "piano",
  output_schema: object({ piano: { type: "string" } }),
});

function mountNode(over: Partial<StepNodeData> = {}, states: Map<string, StepRun> = new Map()) {
  const graph = graphOf([UPSTREAM, WIRED_AND_EMPTY]);
  const full: StepNodeData = {
    step: WIRED_AND_EMPTY,
    kind: "engine",
    flowName: "sviluppa-sailor",
    color: "#000",
    dimmed: false,
    ports: portsOf(graph, WIRED_AND_EMPTY, {}),
    ...over,
  };
  const props = {
    id: "n",
    type: "step",
    data: full,
    selected: false,
    zIndex: 0,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
  } as unknown as NodeProps;
  const { container } = render(
    <StepRunContext.Provider value={states}>
      <StepUsageContext.Provider value={new Map()}>
        <ReactFlowProvider>
          <StepNode {...props} />
        </ReactFlowProvider>
      </StepUsageContext.Provider>
    </StepRunContext.Provider>,
  );
  return container.querySelector(".step-node") as HTMLElement;
}

/** The backdrop a browser would give this element, already opaque. */
function backdropOf(element: Element) {
  const styles = styleTree(document.documentElement, sheet);
  const style = styles.get(element);
  expect(style, "the computed style of a port mark is missing").toBeDefined();
  return (style as { backdrop: Parameters<typeof contrastRatio>[0] }).backdrop;
}

describe("A WIRED PORT AND AN UNWIRED ONE DIFFER WITHOUT COLOR", () => {
  test("the two marks stay far apart in luminance: they survive greyscale", () => {
    const node = mountNode();
    const wired = node.querySelector(".step-node__port[data-wired] .step-node__port-mark");
    const empty = node.querySelector(
      ".step-node__port:not([data-wired]) .step-node__port-mark",
    );
    expect(wired, "no wired port to measure").not.toBeNull();
    expect(empty, "no unwired port to measure").not.toBeNull();

    // 3:1 is the threshold for non-text marks: below it, in black and white, the
    // two become the same blob. Here the filled one is ink and the empty one is
    // the node's paper, so the margin is wide — and it is the margin that has to
    // survive, not the number.
    const ratio = contrastRatio(
      backdropOf(wired as Element),
      backdropOf(empty as Element),
    );
    expect(ratio).toBeGreaterThanOrEqual(3);
  });

  test("empty is really empty: the unwired mark has no fill at all", () => {
    // If someone fills both and hands the difference to the tint, the
    // measurement above collapses — this one says it first, and names the cause.
    const node = mountNode();
    const empty = node.querySelector(
      ".step-node__port:not([data-wired]) .step-node__port-mark",
    ) as Element;
    const styles = styleTree(document.documentElement, sheet);
    const declared = styles.get(empty)?.declarations.get("background");
    expect(parseColor(String(declared))?.a).toBe(0);
  });

  test("«manca» is a WORD, not a tint", () => {
    mountNode();
    expect(screen.getByText("repo manca")).toBeDefined();
  });

  test("the type lives in the shape, and the three shapes are three different drawings", () => {
    const step = stepOf({
      id: "forme",
      input_schema: object({ parola: { type: "string" }, oggetto: object({}), numero: { type: "number" } }),
    });
    const node = mountNode({ step, ports: portsOf(graphOf([step]), step, {}) });
    const styles = styleTree(document.documentElement, sheet);
    const signature = (shape: string) => {
      const mark = node.querySelector(`.step-node__port-mark[data-shape="${shape}"]`);
      expect(mark, `no mark for shape ${shape}`).not.toBeNull();
      const declarations = styles.get(mark as Element)?.declarations as Map<string, string>;
      return [declarations.get("border-radius") ?? "", declarations.get("transform") ?? ""].join("|");
    };
    const all = [signature("text"), signature("structure"), signature("value")];
    expect(new Set(all).size, `two shapes are drawn the same: ${all.join(" · ")}`).toBe(3);
  });
});

describe("state: dot PLUS word", () => {
  const STATES: StepState[] = [
    "waiting",
    "running",
    "went",
    "broke",
    "capped",
    "handed_to_human",
  ];

  test("every state carries a dot and a word, and the words are all different", () => {
    const words = new Set<string>();
    for (const state of STATES) {
      const node = mountNode(
        {},
        new Map([["sviluppa-sailor::implementa", { step_id: "implementa", state, attempt: 1 }]]),
      );
      const label = node.querySelector(".step-node__state") as HTMLElement;
      expect(label.querySelector(".step-node__state-dot"), `${state} has no dot`).not.toBeNull();
      const word = (label.textContent ?? "").trim();
      // The dot alone would be color and nothing else: the word is what survives
      // greyscale, and that is prohibition 5 to the letter.
      expect(word, `${state} has no word`).not.toBe("");
      words.add(word);
      cleanup();
    }
    expect(words.size).toBe(STATES.length);
  });
});

describe("the two registers of attention", () => {
  const run = (state: StepState): StepRun => ({ step_id: "x", state, attempt: 1 });

  test("among three waiting for a person, ONLY ONE is singled out", () => {
    const call = stepThatCallsForAGesture(
      new Map([
        ["f::c", run("handed_to_human")],
        ["f::a", run("handed_to_human")],
        ["f::b", run("capped")],
      ]),
    );
    expect(call.key).toBe("f::a");
    expect(call.waiting).toBe(3);
  });

  test("waiting for a person comes before stopped at the cap", () => {
    const call = stepThatCallsForAGesture(
      new Map([
        ["f::a", run("capped")],
        ["f::z", run("handed_to_human")],
      ]),
    );
    expect(call.key).toBe("f::z");
  });

  test("a live run does NOT ask for attention: nobody singled out", () => {
    // It is the flaw the draft names: every live run asked for attention, which
    // means none of them got it.
    expect(stepThatCallsForAGesture(new Map([["f::a", run("running")]])).key).toBeNull();
    expect(stepThatCallsForAGesture(new Map([["f::a", run("broke")]])).key).toBeNull();
  });

  test("on the canvas the singled-out one is marked, and counts the others in words", () => {
    const node = mountNode(
      {},
      new Map([
        ["sviluppa-sailor::implementa", run("handed_to_human")],
        ["zeta-flusso::coda", run("handed_to_human")],
      ]),
    );
    expect(node.getAttribute("data-calls")).toBe("true");
    expect(screen.getByText("1 more waiting")).toBeDefined();
  });

  test("the second one waiting is NOT singled out", () => {
    const node = mountNode(
      {},
      new Map([
        ["altro-flusso::alfa", run("handed_to_human")],
        ["sviluppa-sailor::implementa", run("handed_to_human")],
      ]),
    );
    expect(node.getAttribute("data-calls")).toBeNull();
  });
});

// ── 3. the sheet and the layout say the same number ─────────────────────

describe("a node's width, and the gap between two nodes", () => {
  test("THE SHEET AND THE LAYOUT SAY THE SAME NUMBER", () => {
    // The class of flaw: `layout.ts` laid the lanes out on a width the node no
    // longer had. No type sees it, and on screen it shows only once two nodes
    // touch each other.
    const rule = sheet.rules.find((candidate) => candidate.selector === ".step-node");
    expect(rule, "the `.step-node` rule is missing").toBeDefined();
    const width = new Map((rule as { declarations: Array<[string, string]> }).declarations).get(
      "width",
    );
    expect(width).toBe(`${STEP_WIDTH}px`);
  });

  test("the declared gap survives between a node and its neighbour", () => {
    // Below this gap a node stops reading as an object and becomes a row: less
    // breathing room than the padding inside the node itself.
    expect(COLUMN - STEP_WIDTH).toBeGreaterThanOrEqual(MIN_NODE_GAP);
  });
});

// ── 4. the promise, measured on the real flows ──────────────────────────

/**
 * **THE THREE SHAPES MUST SHOW ON REAL DATA, OR THE PROMISE IS JUST A WORD.**
 * Every test above builds its own graph, proving the computation right and not
 * that anything is visible — on the sample it was not, since `sample.ts`
 * schemas were all `any` and every port came out square. The thresholds stay
 * loose: the exact count moves whenever somebody touches a flow, which is work
 * and not a defect. What must not move is that all three shapes and both fills
 * exist, in numbers no accident would produce.
 */
describe("the three shapes on the real flows, not on the sample", () => {
  const flows = realFlows();

  test("every flow system.rs ships is read, and the list itself is not empty", () => {
    // Without this, every threshold below could pass on zero files read — the
    // quietest way of being green for having looked at nothing. It used to ask
    // for ten files: a number about a directory, not about this test, and when
    // the flows moved out of the repo a passing test went red. What it names
    // now is read from `system.rs`, so a rename there is not a red here.
    expect(SHIPPED_WITH_THE_BINARY, "system.rs yields no include_str! line").not.toEqual([]);
    // The measurer, measured: a `shippedFlowsMissingFrom` that always answered
    // «nothing missing» would keep both canaries green on an empty directory.
    expect(shippedFlowsMissingFrom([])).toEqual(SHIPPED_WITH_THE_BINARY);
    const paths = readRealFlows().map(({ path }) => path);
    expect(shippedFlowsMissingFrom(paths).join(", "), `read: ${paths.join(", ")}`).toBe("");
  });

  test("CIRCLE, DIAMOND AND SQUARE ARE ALL THERE, AND NONE IS A ONE-OFF", () => {
    const census = portCensus(flows);
    const shapes = `circles ${census.text}, diamonds ${census.structure}, squares ${census.value} of ${census.total}`;
    expect(census.total, `ports read: ${shapes}`).toBeGreaterThan(20);
    // Today, on the three shipped flows: 7 circles, 8 diamonds, 17 squares of
    // 32. Four is below each of them and above what an accident produces: if
    // `shapeOf` collapsed onto a single shape, two of these three would go to
    // zero and the line would say which.
    expect(census.text, shapes).toBeGreaterThanOrEqual(4);
    expect(census.structure, shapes).toBeGreaterThanOrEqual(4);
    expect(census.value, shapes).toBeGreaterThanOrEqual(4);
  });

  test("EMPTY AND FILLED BOTH EXIST: \"which input is missing\" is visible", () => {
    // If every port came out wired, the canvas would be legible and useless: the
    // question the ports exist to answer would never get an answer.
    const census = portCensus(flows);
    // Today, on the three shipped flows: 26 wired, 6 empty of 32. The empty
    // ones are the scarce side, so the floor sits under them with room for an
    // edit: were every port to come out wired, this is the line that would say.
    const fill = `wired ${census.wired}, empty ${census.empty} of ${census.total}`;
    expect(census.wired, fill).toBeGreaterThanOrEqual(10);
    expect(census.empty, fill).toBeGreaterThanOrEqual(3);
  });
});
