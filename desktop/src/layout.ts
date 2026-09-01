import { MarkerType, type Edge, type Node } from "@xyflow/react";
import type { FlowFile, Graph, Step, StepRun, ValueSchema } from "./flow";
import { kindOf } from "./flow";

/**
 * How wide a node is, and the room left between neighbours. A box that sits
 * closer to its neighbour than to its own content reads as a row, not an
 * object. `ports.test.tsx` re-reads the width from `styles.css` and turns red
 * if the two stop agreeing, or if the gap drops under `MIN_NODE_GAP`.
 */
export const STEP_WIDTH = 248;

/** The minimum gap between two side-by-side nodes. Below it they read as a row. */
export const MIN_NODE_GAP = 48;

export const COLUMN = STEP_WIDTH + MIN_NODE_GAP;

/**
 * The gap between two stacked nodes. Equal to the horizontal one: neighbours
 * separate the same way in both directions, or the grid reads as a table with
 * narrow columns.
 */
export const ROW_GAP = MIN_NODE_GAP;

/**
 * The vertical pitch: what a node takes, plus its gap. `ROW - ROW_GAP` is the
 * room before touching the one below, and it is **not a guarantee** — a node
 * has no fixed height, growing with its ports, engine box, token counter and a
 * wrapping header, the tallest measuring 227px. The day one of those has
 * somebody underneath the two touch and no check says so. The cure is stacking
 * on the measured height, not inflating this constant, which would waste space
 * on every short node for the sake of one tall one.
 */
export const ROW = 208;

/**
 * Lays the steps out in levels: a step sits to the right of everything it
 * depends on — the only arrangement in which an arrow never goes backwards.
 * A cycle cannot exist here (`flow::Graph` rejects it at load); if one arrived
 * anyway, unresolved steps land at the end instead of spinning the loop.
 */
export function depths(graph: Graph): Map<string, number> {
  const known = new Map<string, number>();
  const byId = new Map(graph.steps.map((step) => [step.id, step]));
  let progressed = true;

  while (progressed && known.size < graph.steps.length) {
    progressed = false;
    for (const step of graph.steps) {
      if (known.has(step.id)) continue;
      const ready = step.deps.every((dep) => known.has(dep));
      if (!ready) continue;
      const depth = step.deps.reduce(
        (deepest, dep) => Math.max(deepest, (known.get(dep) ?? 0) + 1),
        0,
      );
      known.set(step.id, depth);
      progressed = true;
    }
  }

  // Whatever did not resolve: at the end, visible, never hidden.
  const last = Math.max(0, ...known.values()) + 1;
  for (const step of graph.steps) {
    if (!known.has(step.id)) known.set(step.id, last);
    void byId;
  }
  return known;
}

export function toNodes(graph: Graph, runs: Map<string, StepRun>): Node[] {
  const depth = depths(graph);
  const perColumn = new Map<number, number>();

  return graph.steps.map((step) => {
    const column = depth.get(step.id) ?? 0;
    const row = perColumn.get(column) ?? 0;
    perColumn.set(column, row + 1);

    return {
      id: step.id,
      type: "step",
      position: { x: column * COLUMN, y: row * ROW },
      data: {
        step,
        kind: kindOf(step.action),
        run: runs.get(step.id),
        ports: portsOf(graph, step, {}),
      },
    };
  });
}

export function toEdges(graph: Graph, runs: Map<string, StepRun> = new Map()): Edge[] {
  const skippable = new Set(
    (graph.skippable_dependencies ?? []).map(
      (edge) => `${edge.step}<-${edge.dependency}`,
    ),
  );

  return graph.steps.flatMap((step) =>
    step.deps.map((dependency) => {
      const optional = skippable.has(`${step.id}<-${dependency}`);
      const look = edgeLook(optional, runs.get(dependency), runs.get(step.id));
      return edgeFrom(`${dependency}->${step.id}`, dependency, step.id, look, 1);
    }),
  );
}

// ── the single canvas: every flow, branches of one system ───────────────
//
// No flow declares a dependency on a step of another flow, so an arc between
// two flows would be an invented arc. What can honestly be done is showing them
// as branches of one tree: one canvas, each flow in its own coloured lane, and
// no bridges that do not exist on disk.

const BAND_GAP = 56;
const BAND_PAD_X = 28;

/**
 * The height reserved for a lane's heading, before the first node. **IT DEPENDS
 * ON THE STYLESHEET, AND NOTHING IN TYPESCRIPT KNOWS THAT**: below `FAR_ZOOM`
 * the labels grow, and 0.5 is the zoom the canvas picks on opening.
 * `layout.test.tsx` redoes the sum by reading `styles.css`.
 */
export const BAND_PAD_TOP = 88;

/** How much air must be left between the description and the first node. */
export const BAND_HEAD_GAP = 8;

/** A lane's description is clamped to two lines by `styles.css`. */
export const BAND_DESC_LINES = 2;

/**
 * The air at the bottom of a lane, and the tolerance for whoever overflows. The
 * last row of nodes does not carry its own gap: without this subtraction a
 * one-row lane would stand as tall as a two-row one. Put back as padding, it
 * also leaves room for a node taller than `ROW - ROW_GAP`.
 */
const BAND_PAD_BOTTOM = 40;

/**
 * The tint of a lane, taken by index and cycling.
 *
 * A LANE HAS NO ROLES OF ITS OWN YET, so these borrow the roles that exist and
 * are already measured. Six tints for ten flows on this machine means four
 * collisions — which is why the lane carries its name next to the tint, and why
 * the tint never decides anything by itself.
 */
const LANE_TINTS = [
  "var(--state-running)",
  "var(--state-went)",
  "var(--state-capped)",
  "var(--state-broke)",
  "var(--state-human)",
  "var(--muted)",
];

export function colorForFlow(index: number): string {
  return LANE_TINTS[index % LANE_TINTS.length];
}

/** How a cord is drawn: the tint, whether it is broken, whether it is alive. */
export interface EdgeLook {
  stroke: string;
  dash?: string;
  live: boolean;
}

/**
 * What a cord says, read from the two steps it joins.
 *
 * The path a run actually took is a full green line; the one it has not reached
 * is a quiet thin one; a dependency the graph declares skippable is broken,
 * because its data may never arrive. The dash carries that on its own, so
 * greyscale loses nothing (prohibition 5).
 */
export function edgeLook(
  optional: boolean,
  from: StepRun | undefined,
  to: StepRun | undefined,
): EdgeLook {
  const dash = optional ? "5 4" : undefined;
  if (to?.state === "running") return { stroke: "var(--state-running)", dash: "8 6", live: true };
  const taken = from?.state === "went" && to !== undefined && to.state !== "waiting";
  if (taken) return { stroke: "var(--state-went)", dash, live: false };
  return { stroke: optional ? "var(--warn)" : "var(--line)", dash, live: false };
}

function edgeFrom(
  id: string,
  source: string,
  target: string,
  look: EdgeLook,
  opacity: number,
): Edge {
  return {
    id,
    source,
    target,
    // Rounded orthogonal: on a graph laid out in columns a cord that changes row
    // has to be followed with the eye, and a bezier crossing three nodes cannot.
    type: "smoothstep",
    animated: look.live,
    markerEnd: { type: MarkerType.ArrowClosed, color: look.stroke, width: 16, height: 16 },
    style: { stroke: look.stroke, strokeWidth: 1.5, strokeDasharray: look.dash, opacity },
  } satisfies Edge;
}

// ── a step's ports ──────────────────────────────────────────────────────
//
// **TYPE IS IN THE SHAPE, WIRING IS IN THE FILL.** Three shapes — circle text,
// diamond struct, square value — and a port is hollow when nothing feeds it.
// So "which input is missing" reads off the canvas, with no error badge.

// THE THREE SHAPES ARE THREE `ValueSchema` FAMILIES, NOT THREE INVENTIONS. The
// schema language has NO file type, so drawing one would promise a distinction
// the engine does not make. The third square therefore carries the scalars.

// WHERE "WIRED" COMES FROM, as `step_input` composes it in `executor.rs`.
// Anything matching none of the four is fed by nothing:
//  - `step.with[name]`  -> written in there, and it beats everything;
//  - no dependency      -> from `inputs[step-id]` of the flow file;
//  - one required dependency -> the input IS that dependency's output;
//  - several            -> an object with one key per dependency.

// **THERE IS A FIFTH FEEDER AND `portsOf` DOES NOT KNOW IT: `workdir`.** After
// `overlay_input`, `resolve_workdir` puts the workspace root into the input of
// EVERY step whose schema accepts it (`accepts_property` in
// `crates/flow/src/schema.rs`). A step declaring `workdir` in its own
// `input_schema` without writing it in `with` would get it from the engine
// while this port showed hollow — a false accusation.

// It is a debt and not yet a defect, and that is measured: no real flow
// declares `workdir` in an input schema. Whoever writes the first one must add
// the case here, before the canvas accuses the engine of not doing its job.

/** The three shapes of a port. The tint is only redundancy. */
export type PortShape = "text" | "structure" | "value";

/**
 * Where a port's value comes from. `unknown` is not `none`, the same discipline
 * the node already uses for the tool: when the dependency declares `any`, which
 * keys it will produce is NOT KNOWN, and saying "missing" would be an invented
 * accusation.
 */
export type PortFeed = "fixed" | "upstream" | "flow" | "unknown" | "none";

export interface StepPort {
  /** The name written in the file: it is data, and it stays as it is. */
  name: string;
  shape: PortShape;
  /** True when something really feeds this port. */
  wired: boolean;
  /** True when the schema declares it required. */
  required: boolean;
  feed: PortFeed;
}

export interface StepPorts {
  inputs: StepPort[];
  output: StepPort;
}

/** The input of a step that depends on nobody: the flow file opens it. */
export const ROOT_PORT_NAME = "avvio";

/** A step's single output. Hollow when nobody reads it. */
export const OUTPUT_PORT_NAME = "uscita";

export function shapeOf(schema: ValueSchema | undefined): PortShape {
  if (schema === undefined) return "value";
  switch (schema.type) {
    case "string":
      return "text";
    case "object":
    case "array":
      return "structure";
    // A choice between texts is still text: the shape a viewer recognises.
    case "one_of":
      return schema.values.every((value) => typeof value === "string") ? "text" : "value";
    default:
      return "value";
  }
}

/** The properties a schema declares, or nothing if it is not an object. */
function objectSchema(
  schema: ValueSchema | undefined,
): { properties: Record<string, ValueSchema>; required: string[] } | null {
  if (schema === undefined || schema.type !== "object") return null;
  return { properties: schema.properties, required: schema.required };
}

function isSkippable(graph: Graph, step: string, dependency: string): boolean {
  return (graph.skippable_dependencies ?? []).some(
    (edge) => edge.step === step && edge.dependency === dependency,
  );
}

/**
 * The names the engine will really put in this step's input, or `null` when the
 * dependency does not declare its own keys.
 */
function suppliedNames(
  graph: Graph,
  step: Step,
  flowInputs: Record<string, unknown>,
): Set<string> | null {
  const byId = new Map(graph.steps.map((other) => [other.id, other]));
  const deps = step.deps;
  if (deps.length === 0) {
    const opening = flowInputs[step.id];
    if (opening === null || typeof opening !== "object" || Array.isArray(opening)) {
      return new Set();
    }
    return new Set(Object.keys(opening as Record<string, unknown>));
  }
  if (deps.length === 1 && !isSkippable(graph, step.id, deps[0])) {
    const produced = objectSchema(byId.get(deps[0])?.output_schema);
    return produced === null ? null : new Set(Object.keys(produced.properties));
  }
  return new Set(deps);
}

/**
 * A step's ports, read from the graph and the file — never invented. When the
 * input schema declares nothing (`any`, over half the real steps) the node does
 * not go silent: one port per dependency, or the start port if it has none. A
 * step with neither gets `null`, and the hollow port says so.
 */
export function portsOf(
  graph: Graph,
  step: Step,
  flowInputs: Record<string, unknown>,
): StepPorts {
  const byId = new Map(graph.steps.map((other) => [other.id, other]));
  const supplied = suppliedNames(graph, step, flowInputs);
  const declared = objectSchema(step.input_schema);
  const inputs: StepPort[] = [];

  if (declared !== null) {
    for (const [name, schema] of Object.entries(declared.properties)) {
      const fixed = step.with != null && Object.hasOwn(step.with, name);
      const feed: PortFeed = fixed
        ? "fixed"
        : supplied === null
          ? "unknown"
          : supplied.has(name)
            ? step.deps.length === 0
              ? "flow"
              : "upstream"
            : "none";
      inputs.push({
        name,
        shape: shapeOf(schema),
        wired: feed !== "none",
        required: declared.required.includes(name),
        feed,
      });
    }
  } else if (step.deps.length > 0) {
    for (const dependency of step.deps) {
      inputs.push({
        name: dependency,
        shape: shapeOf(byId.get(dependency)?.output_schema),
        wired: true,
        // A skippable dependency already promises its datum may be missing.
        required: !isSkippable(graph, step.id, dependency),
        feed: "upstream",
      });
    }
  } else {
    const opened = flowInputs[step.id] !== undefined;
    inputs.push({
      name: ROOT_PORT_NAME,
      shape: shapeOf(step.input_schema),
      wired: opened,
      required: false,
      feed: opened ? "flow" : "none",
    });
  }

  const consumed = graph.steps.some((other) => other.deps.includes(step.id));
  return {
    inputs,
    output: {
      name: OUTPUT_PORT_NAME,
      shape: shapeOf(step.output_schema),
      wired: consumed,
      required: false,
      feed: consumed ? "upstream" : "none",
    },
  };
}

/**
 * A node belongs to a flow: the id carries the flow name in front, because two
 * steps of different flows can share a name and the canvas is a single one.
 */
export function nodeId(flowName: string, stepId: string): string {
  return `${flowName}::${stepId}`;
}

export function splitNodeId(id: string): { flowName: string; stepId: string } {
  const separator = id.indexOf("::");
  return { flowName: id.slice(0, separator), stepId: id.slice(separator + 2) };
}

/**
 * True if wiring `from -> to` (making `to` depend on `from`) closes a cycle —
 * that is, if `from` already depends, even indirectly, on `to`.
 */
export function wouldCycle(graph: Graph, from: string, to: string): boolean {
  const byId = new Map(graph.steps.map((step) => [step.id, step]));
  const stack = [from];
  const seen = new Set<string>();
  while (stack.length > 0) {
    const current = stack.pop() as string;
    if (current === to) return true;
    if (seen.has(current)) continue;
    seen.add(current);
    const step = byId.get(current);
    if (step) stack.push(...step.deps);
  }
  return false;
}

export interface FlowBand {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
}

export interface UnifiedLayout {
  nodes: Node[];
  edges: Edge[];
  /** Each flow's box, in canvas coordinates: used to focus a branch. */
  bands: Map<string, FlowBand>;
}

/**
 * Lays every flow out on one canvas, one horizontal lane per flow, stacked top
 * to bottom. Inside a lane the level order of `toNodes`/`toEdges` applies; no
 * arc crosses between lanes, for the reason written above.
 */
export function buildUnifiedLayout(
  flows: Array<{ name: string; flow: FlowFile }>,
  runs: Map<string, StepRun>,
  focus: string | null,
): UnifiedLayout {
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  const bands = new Map<string, FlowBand>();
  let top = 0;

  flows.forEach(({ name, flow }, index) => {
    const graph = flow.graph;
    const depth = depths(graph);
    const color = colorForFlow(index);
    const dimmed = focus !== null && focus !== name;

    let maxColumn = 0;
    const perColumn = new Map<number, number>();
    for (const step of graph.steps) {
      const column = depth.get(step.id) ?? 0;
      maxColumn = Math.max(maxColumn, column);
      perColumn.set(column, (perColumn.get(column) ?? 0) + 1);
    }
    const maxRows = Math.max(1, ...perColumn.values());
    const width = maxColumn * COLUMN + STEP_WIDTH + BAND_PAD_X * 2;
    const height = maxRows * ROW - ROW_GAP + BAND_PAD_TOP + BAND_PAD_BOTTOM;

    bands.set(name, { x: 0, y: top, width, height, color });
    nodes.push({
      id: `band::${name}`,
      type: "flowBand",
      position: { x: 0, y: top },
      draggable: false,
      selectable: false,
      zIndex: -1,
      style: { width, height },
      data: {
        name,
        description: flow.description,
        stepCount: graph.steps.length,
        color,
        dimmed,
      },
    });

    const placed = new Map<number, number>();
    for (const step of graph.steps) {
      const column = depth.get(step.id) ?? 0;
      const row = placed.get(column) ?? 0;
      placed.set(column, row + 1);
      nodes.push({
        id: nodeId(name, step.id),
        type: "step",
        position: { x: BAND_PAD_X + column * COLUMN, y: top + BAND_PAD_TOP + row * ROW },
        data: {
          step,
          kind: kindOf(step.action),
          run: runs.get(step.id),
          flowName: name,
          color,
          dimmed,
          // Ports live in `data` and not in a context because they depend only
          // on the file, not on a run. What this canvas cannot take is
          // rebuilding the node list on every incoming FACT; the file changes
          // only when somebody edits it, which is exactly when the ports must.
          ports: portsOf(graph, step, flow.inputs ?? {}),
        },
      });
    }

    const skippable = new Set(
      (graph.skippable_dependencies ?? []).map((edge) => `${edge.step}<-${edge.dependency}`),
    );
    for (const step of graph.steps) {
      for (const dependency of step.deps) {
        const optional = skippable.has(`${step.id}<-${dependency}`);
        // THE CORD IS COLOURED BY THE OUTCOME, NOT BY THE LANE. Which flow a
        // cord belongs to is already said by the band it lies in; what was said
        // nowhere is whether the run came through here.
        const look = edgeLook(optional, runs.get(dependency), runs.get(step.id));
        edges.push(
          edgeFrom(
            `${nodeId(name, dependency)}->${nodeId(name, step.id)}`,
            nodeId(name, dependency),
            nodeId(name, step.id),
            look,
            dimmed ? 0.25 : 1,
          ),
        );
      }
    }

    top += height + BAND_GAP;
  });

  return { nodes, edges, bands };
}
