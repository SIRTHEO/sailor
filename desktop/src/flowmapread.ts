/**
 * **WHICH FLOW CALLS WHICH.** Nodes are flows, edges are the steps that run
 * another flow — `subflow` and `for_each`, the two actions that name one in
 * `with.flow`. Derived from the files themselves so the map cannot drift from
 * what the engine would resolve, shadowing included.
 */

/** The two actions whose `with.flow` is the name of another flow on disk. */
const CALLING_ACTIONS = ["subflow", "for_each"] as const;

/** Where a file came from: what Sailor ships, or what the person wrote. */
export type FlowOrigin = "shipped" | "own";

export interface FlowFileOnDisk {
  /** The file name as it reads on disk, `.flow.json` and all. */
  name: string;
  origin: FlowOrigin;
  text: string;
  /**
   * Why the file would not open, when it would not. **A FILE NOBODY COULD
   * OPEN IS NOT A FILE WITH NO TEXT IN IT**: read as empty it reports back as
   * malformed JSON, which blames the author for a permission or a broken link.
   */
  unreadable?: string;
}

export interface FlowCall {
  /** The called flow's name, whether or not a file answers to it. */
  to: string;
  /** The step that makes the call, by its id. */
  step: string;
  /** Which of [`CALLING_ACTIONS`] the step declares. */
  via: string;
  missing: boolean;
}

export interface BrokenCall {
  from: string;
  to: string;
  step: string;
  via: string;
}

export interface UnreadFile {
  name: string;
  why: string;
}

export interface FlowNode {
  /** The name the engine resolves by: the file name without `.flow.json`. */
  id: string;
  description: string;
  origin: FlowOrigin;
  calls: FlowCall[];
  calledBy: string[];
  /** Nothing outside its own ring calls it, so a person starts runs here. */
  entry: boolean;
  inCycle: boolean;
  /** How far below the entries it sits, rings counted as one step. */
  layer: number;
  /** Its place inside its layer, so two readings draw the same picture. */
  row: number;
}

export interface FlowMapReading {
  nodes: FlowNode[];
  /** Each ring's members, named once and sorted; a self-call is a ring of one. */
  cycles: string[][];
  broken: BrokenCall[];
  unread: UnreadFile[];
  /** Names a later source overrode, which is how the engine resolves them. */
  shadowed: string[];
  shipped: number;
  own: number;
}

interface Parsed {
  id: string;
  description: string;
  origin: FlowOrigin;
  calls: { to: string; step: string; via: string }[];
}

function stemOf(name: string): string {
  return name.replace(/\.flow\.json$/, "");
}

function callsIn(body: unknown): { to: string; step: string; via: string }[] {
  const steps = (body as { graph?: { steps?: unknown } })?.graph?.steps;
  if (!Array.isArray(steps)) return [];
  const found: { to: string; step: string; via: string }[] = [];
  for (const raw of steps) {
    const step = raw as { id?: unknown; action?: unknown; with?: { flow?: unknown } };
    const via = String(step.action ?? "");
    if (!CALLING_ACTIONS.some((known) => known === via)) continue;
    const to = step.with?.flow;
    if (typeof to !== "string" || to === "") continue;
    found.push({ to, step: String(step.id ?? ""), via });
  }
  return found;
}

/**
 * Read each file once, keeping the last of a name: order is the engine's only
 * precedence rule, so a later source shadows an earlier one rather than
 * doubling it.
 */
function parseAll(files: FlowFileOnDisk[]): { byId: Map<string, Parsed>; unread: UnreadFile[]; shadowed: string[] } {
  const byId = new Map<string, Parsed>();
  const unread: UnreadFile[] = [];
  const shadowed: string[] = [];
  for (const file of files) {
    const id = stemOf(file.name);
    if (file.unreadable !== undefined) {
      unread.push({ name: file.name, why: file.unreadable });
      continue;
    }
    let body: unknown;
    try {
      body = JSON.parse(file.text);
    } catch (error) {
      unread.push({ name: file.name, why: String(error) });
      continue;
    }
    if (byId.has(id)) shadowed.push(id);
    const said = (body as { description?: unknown })?.description;
    byId.set(id, {
      id,
      description: typeof said === "string" ? said : "",
      origin: file.origin,
      calls: callsIn(body),
    });
  }
  return { byId, unread, shadowed: [...new Set(shadowed)].sort() };
}

/**
 * Tarjan's strongly connected components, iterative so a deep graph cannot
 * overflow the stack. Every component comes back, one-member ones included:
 * whether a lone member is a ring is decided by its own edges.
 */
function components(ids: string[], edges: Map<string, string[]>): string[][] {
  const index = new Map<string, number>();
  const low = new Map<string, number>();
  const onStack = new Set<string>();
  const stack: string[] = [];
  const found: string[][] = [];
  let next = 0;

  for (const root of ids) {
    if (index.has(root)) continue;
    const work: { at: string; child: number }[] = [{ at: root, child: 0 }];
    index.set(root, next);
    low.set(root, next);
    next += 1;
    stack.push(root);
    onStack.add(root);
    while (work.length > 0) {
      const frame = work[work.length - 1];
      const children = edges.get(frame.at) ?? [];
      if (frame.child < children.length) {
        const child = children[frame.child];
        frame.child += 1;
        if (!index.has(child)) {
          index.set(child, next);
          low.set(child, next);
          next += 1;
          stack.push(child);
          onStack.add(child);
          work.push({ at: child, child: 0 });
        } else if (onStack.has(child)) {
          low.set(frame.at, Math.min(low.get(frame.at) ?? 0, index.get(child) ?? 0));
        }
        continue;
      }
      work.pop();
      const parent = work[work.length - 1];
      if (parent) low.set(parent.at, Math.min(low.get(parent.at) ?? 0, low.get(frame.at) ?? 0));
      if (low.get(frame.at) === index.get(frame.at)) {
        const group: string[] = [];
        for (;;) {
          const member = stack.pop();
          if (member === undefined) break;
          onStack.delete(member);
          group.push(member);
          if (member === frame.at) break;
        }
        found.push(group.sort());
      }
    }
  }
  return found;
}

/**
 * How far each component sits below the entries: zero when nothing outside
 * calls it, otherwise one past the deepest caller. Computed over the rings
 * rather than the flows, which is what keeps a cycle from having no answer.
 */
function layers(groups: string[][], home: Map<string, number>, edges: Map<string, string[]>): number[] {
  const into: number[][] = groups.map(() => []);
  for (const [from, targets] of edges) {
    const source = home.get(from);
    if (source === undefined) continue;
    for (const to of targets) {
      const sink = home.get(to);
      if (sink === undefined || sink === source) continue;
      into[sink].push(source);
    }
  }
  const depth: number[] = groups.map(() => -1);
  const settle = (at: number, seen: Set<number>): number => {
    if (depth[at] >= 0) return depth[at];
    if (seen.has(at)) return 0;
    seen.add(at);
    let deepest = -1;
    for (const from of into[at]) deepest = Math.max(deepest, settle(from, seen));
    seen.delete(at);
    depth[at] = deepest + 1;
    return depth[at];
  };
  for (let at = 0; at < groups.length; at += 1) settle(at, new Set());
  return depth;
}

/** The whole map, from the files to the picture, in one pass a caller repeats. */
export function readFlowMap(files: FlowFileOnDisk[]): FlowMapReading {
  const { byId, unread, shadowed } = parseAll(files);
  const ids = [...byId.keys()].sort();
  const edges = new Map<string, string[]>();
  for (const id of ids) {
    edges.set(id, [...new Set(byId.get(id)?.calls.map((call) => call.to) ?? [])].filter((to) => byId.has(to)));
  }

  const broken: BrokenCall[] = [];
  const calledBy = new Map<string, Set<string>>();
  for (const id of ids) calledBy.set(id, new Set());
  const callsOf = new Map<string, FlowCall[]>();
  for (const id of ids) {
    const made: FlowCall[] = [];
    for (const call of byId.get(id)?.calls ?? []) {
      const missing = !byId.has(call.to);
      if (missing) broken.push({ from: id, to: call.to, step: call.step, via: call.via });
      else calledBy.get(call.to)?.add(id);
      made.push({ ...call, missing });
    }
    made.sort((left, right) => left.to.localeCompare(right.to) || left.step.localeCompare(right.step));
    callsOf.set(id, made);
  }

  const groups = components(ids, edges);
  const home = new Map<string, number>();
  groups.forEach((group, at) => group.forEach((member) => home.set(member, at)));
  const ring = groups.map((group) => group.length > 1 || (edges.get(group[0]) ?? []).includes(group[0]));
  const depth = layers(groups, home, edges);

  const rows = new Map<number, number>();
  const nodes: FlowNode[] = ids
    .map((id) => {
      const at = home.get(id) ?? 0;
      const parsed = byId.get(id) as Parsed;
      const outside = [...(calledBy.get(id) ?? [])].filter((caller) => home.get(caller) !== at);
      return {
        id,
        description: parsed.description,
        origin: parsed.origin,
        calls: callsOf.get(id) ?? [],
        calledBy: [...(calledBy.get(id) ?? [])].sort(),
        entry: outside.length === 0 && groups[at][0] === id,
        inCycle: ring[at],
        layer: depth[at],
        row: 0,
      };
    })
    .sort((left, right) => left.layer - right.layer || left.id.localeCompare(right.id));
  for (const node of nodes) {
    const taken = rows.get(node.layer) ?? 0;
    node.row = taken;
    rows.set(node.layer, taken + 1);
  }

  broken.sort((left, right) => left.from.localeCompare(right.from) || left.to.localeCompare(right.to));
  const cycles = groups.filter((_, at) => ring[at]).map((group) => [...group]).sort((left, right) => left[0].localeCompare(right[0]));
  return {
    nodes,
    cycles,
    broken,
    unread,
    shadowed,
    shipped: nodes.filter((node) => node.origin === "shipped").length,
    own: nodes.filter((node) => node.origin === "own").length,
  };
}
