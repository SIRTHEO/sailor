import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import {
  FlowBandNode,
  StepNode,
  STATE_COLOR,
  StepRunContext,
  StepUsageContext,
  WireContext,
  type StepNodeData,
} from "./StepNode";
import { stepUsageOfRun, type StepUsage } from "./stepusage";
import { stepStatesOfCanvas, stepStatesOfRun } from "./runstate";
import { BlankCanvas } from "./BlankCanvas";
import { Now } from "./Now";
import { History, lastedOf, outcomeOf, whenOf } from "./History";
import { useAsk, useClock } from "./ask";
import { Installed } from "./Installed";
import { Manual } from "./Manual";
import { Terminals } from "./Terminals";
import { StepEditor } from "./StepEditor";
import { StepLive } from "./StepLive";
import { ProfileList } from "./ProfileList";
import { Projects } from "./Projects";
import { Worktrees } from "./Worktrees";
import { WireMenu } from "./WireMenu";
import { withStepWiredTo } from "./wiring";
import { Toolbar } from "./Toolbar";
import { RunContext, TriggerNode, triggerNodeId, type RunControls, type TriggerState } from "./TriggerNode";
import { RunConsole, type ConsoleMode } from "./RunConsole";
import { StepHistory } from "./StepHistory";
import { buildUnifiedLayout, nodeId, splitNodeId, wouldCycle } from "./layout";
import { SAMPLE, SAMPLE_RUN } from "./sample";
import {
  deleteFlow,
  discoverTools,
  executionHistory,
  flowTrigger,
  insideTheWindow,
  knownRuns,
  listenToRuns,
  loadFlows,
  runSnapshot,
  runUsage,
  saveFlow,
  startRun,
  type Execution,
  type RunEvent,
  type RunSnapshot,
} from "./engine";
import {
  DEFAULT_ACTION_FOR_KIND,
  KNOWN_ACTIONS,
  stepCountLabel,
  type BrokenFlow,
  type FlowEntry,
  type FlowFile,
  type Origin,
  type RunUsage,
  type Step,
  type StepKind,
  type StepRun,
} from "./flow";
import { MODEL_KEY, type Tool, type ToolDiscovery } from "./tools";

const nodeTypes = { step: StepNode, flowBand: FlowBandNode, trigger: TriggerNode };

/** How much room the trigger takes to the left of its lane. */
const TRIGGER_WIDTH = 240;
const TRIGGER_GAP = 48;
const TRIGGER_TOP = 54;

/**
 * Inside the shell or in a browser: decided once, at startup. It changes what
 * the canvas shows before the first load even runs.
 */
const NATIVE = insideTheWindow();

/** Where the flows on screen came from. */
type Source = "loading" | "sample" | "engine" | "failed";

/**
 * The places of the window. **IT OPENS ON "NOW", NOT ON THE CANVAS**: opening
 * on the inventory answers "what could I run", while whoever reopens the window
 * is asking "what is happening". The canvas is where you go to look inside.
 */
type Place =
  | "now"
  | "history"
  | "flows"
  | "installed"
  | "manual"
  | "terminals"
  | "workspaces"
  | "profiles"
  | "worktrees";

/**
 * Only the graph: "Code" was a data file dressed as source, and "Runs" is
 * already the «Adesso» and «Cronologia» places. `FlowCode` and `FlowRuns` stay
 * below, unmounted, with what they measured about the engine.
 */
type FlowView = "graph";

/**
 * A flow being edited: what is on screen and what is already on disk.
 * `saved: null` means "the disk does not know it", not "identical to the
 * disk" — the two lead to different gestures (it always saves; it deletes
 * without asking the engine anything).
 */
interface WorkingFlow {
  flow: FlowFile;
  saved: FlowFile | null;
  /**
   * Where this flow comes from, as the engine names it — `null` while it has
   * never touched a disk. Not decoration: three sources reach the window and
   * the most specific one wins on a name clash, so the origin is also the
   * answer to «which of the two with this name actually runs».
   */
  origin: Origin | null;
}

function isDirty(working: WorkingFlow): boolean {
  if (working.saved === null) return true;
  return JSON.stringify(working.flow) !== JSON.stringify(working.saved);
}

/**
 * A flow that will not load, and the place it will not load from. The place
 * matters more here than anywhere: «one flow is broken» sends you looking, and
 * without the origin the search starts with «in which of the three folders».
 */
type BrokenAt = BrokenFlow & { origin: Origin };

/** Splits the input into editable flows (map by name) and broken flows (list). */
function splitEntries(entries: FlowEntry[]): { flows: Map<string, WorkingFlow>; broken: BrokenAt[] } {
  const flows = new Map<string, WorkingFlow>();
  const broken: BrokenAt[] = [];
  for (const entry of entries) {
    if (entry.state === "loaded") {
      flows.set(entry.flow.id, { flow: entry.flow, saved: entry.flow, origin: entry.origin });
    } else {
      broken.push({ ...entry.broken, origin: entry.origin });
    }
  }
  return { flows, broken };
}

/** What the column writes over a group of flows that has no origin yet. */
const UNSAVED_GROUP = "not saved yet";

/**
 * The column's flows, gathered under the place each comes from. **THE ORDER IS
 * THE ENGINE'S, NOT AN ALPHABET**: `flow_sources` gives the least specific
 * first and the last wins a name clash, so sorted here the column would stop
 * matching what runs. The unsaved come last, belonging to no disk.
 */
function groupByOrigin(
  flows: { name: string; flow: FlowFile; origin: Origin | null }[],
  broken: BrokenAt[],
): { origin: Origin | null; flows: { name: string; flow: FlowFile }[]; broken: BrokenAt[] }[] {
  const groups = new Map<string, { origin: Origin | null; flows: { name: string; flow: FlowFile }[]; broken: BrokenAt[] }>();
  const group = (origin: Origin | null) => {
    const key = origin ?? UNSAVED_GROUP;
    const existing = groups.get(key);
    if (existing) return existing;
    const fresh = { origin, flows: [], broken: [] };
    groups.set(key, fresh);
    return fresh;
  };
  for (const entry of flows) group(entry.origin).flows.push({ name: entry.name, flow: entry.flow });
  for (const entry of broken) group(entry.origin).broken.push(entry);
  return [...groups.values()].sort(
    (a, b) => Number(a.origin === null) - Number(b.origin === null),
  );
}

/**
 * Merges a run's facts by sequence number. A fact can arrive twice — from the
 * backlog and from the live feed — and the two channels overlap exactly when
 * the view opens, so `seq` is the only way not to depend on which wins. With
 * nothing new it returns the original array and the redraw is skipped.
 */
function mergeEvents(existing: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  const fresh = incoming.filter(
    (event) => !existing.some((known) => known.seq === event.seq),
  );
  if (fresh.length === 0) return existing;
  return [...existing, ...fresh].sort((a, b) => a.seq - b.seq);
}

/** A free name for a new flow: numbered, never colliding. */
function freeFlowName(taken: Map<string, WorkingFlow>): string {
  let n = 1;
  while (taken.has(`flusso-${n}`)) n += 1;
  return `flusso-${n}`;
}

export default function App() {
  // THE CANVAS STARTS AS THE DISK, NOT AS A SAMPLE. Inside the shell it starts
  // empty and waits for the engine: flashing the sample first shows flows that
  // are not on disk. Outside the shell there is no engine and the sample is
  // all there is — the bar always declares which of the two you are looking at.
  const [flows, setFlows] = useState<Map<string, WorkingFlow>>(() =>
    NATIVE ? new Map<string, WorkingFlow>() : splitEntries(SAMPLE).flows,
  );
  const [broken, setBroken] = useState<BrokenAt[]>(() => (NATIVE ? [] : splitEntries(SAMPLE).broken));
  const [source, setSource] = useState<Source>(NATIVE ? "loading" : "sample");
  const [failure, setFailure] = useState<string | null>(null);

  const [place, setPlace] = useState<Place>("now");
  const [flowView] = useState<FlowView>("graph");

  // Focus belongs to the branch, not the canvas: the rail points at a path
  // inside the single graph, it does not choose which graph to show.
  const [focusName, setFocusName] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);

  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});

  const [discovery, setDiscovery] = useState<ToolDiscovery>(() =>
    NATIVE ? { state: "asking" } : { state: "mute", why: "fuori dal guscio: gli strumenti li conosce il motore" },
  );

  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    loadFlows()
      .then((loaded) => {
        if (dropped) return;
        const split = splitEntries(loaded);
        setFlows(split.flows);
        setBroken(split.broken);
        setSource("engine");
        setFocusName(null);
        setSelectedNode(null);
      })
      .catch((error: unknown) => {
        if (dropped) return;
        // A silent engine is not papered over with the sample: that would show
        // flows that are not on disk.
        setSource("failed");
        setFailure(String(error));
      });
    return () => {
      dropped = true;
    };
  }, []);

  // TOOLS ARE ASKED FOR, NOT KNOWN. If `discover_tools` does not answer, the
  // panel says so and lets the id be typed by hand — no blank screen, and no
  // fake list to make it look as though it works.
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    discoverTools()
      .then((found) => {
        if (dropped) return;
        setDiscovery({ state: "ready", tools: found });
      })
      .catch((error: unknown) => {
        if (dropped) return;
        setDiscovery({ state: "mute", why: String(error) });
      });
    return () => {
      dropped = true;
    };
  }, []);

  const tools = discovery.state === "ready" ? discovery.tools : EMPTY_TOOLS;


  const anyDirty = useMemo(() => Array.from(flows.values()).some(isDirty), [flows]);

  // `beforeunload` IS AN HONEST FALLBACK, NOT THE DEFENCE. In the native shell
  // closing is a window event, not a page navigation, so this listener may
  // never be consulted. What holds regardless is the "unsaved" badge, always
  // visible in the bar and beside every flow.
  useEffect(() => {
    function handleBeforeUnload(event: BeforeUnloadEvent) {
      if (!anyDirty) return;
      event.preventDefault();
      event.returnValue = "";
    }
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [anyDirty]);

  const flowList = useMemo(
    () =>
      Array.from(flows.entries()).map(([name, working]) => ({
        name,
        flow: working.flow,
        origin: working.origin,
      })),
    [flows],
  );

  /** The column's contents, under the place each flow comes from. */
  const railGroups = useMemo(() => groupByOrigin(flowList, broken), [flowList, broken]);

  // The models already written in the other steps: the most honest suggestion
  // there is, because it comes from real flows instead of an invented list.
  const usedModels = useMemo(() => {
    const seen = new Set<string>();
    for (const { flow } of flowList) {
      for (const step of flow.graph.steps) {
        const model = step.with?.[MODEL_KEY];
        if (typeof model === "string" && model !== "") seen.add(model);
      }
    }
    return Array.from(seen);
  }, [flowList]);

  // ── runs: starting one, and watching it ───────────────────────────────
  //
  // RUNS DO NOT LIVE HERE. They live in the shell, on a thread of their own;
  // this map is the copy the window keeps in order to draw them. Hence closing
  // the view or reloading the page stops nothing, and hence the first thing to
  // do on mount is ask the shell what is already running.
  const [executions, setExecutions] = useState<Map<string, RunSnapshot>>(new Map());
  const [triggers, setTriggers] = useState<Map<string, TriggerState>>(new Map());
  const [mandates, setMandates] = useState<Record<string, string>>({});
  const [starting, setStarting] = useState<Set<string>>(new Set());
  const [runErrors, setRunErrors] = useState<Record<string, string>>({});
  const [watching, setWatching] = useState<string | null>(null);
  const [consoleMode, setConsoleMode] = useState<ConsoleMode>("inline");

  function absorb(snapshot: RunSnapshot) {
    setExecutions((prev) => {
      const current = prev.get(snapshot.run_id);
      const next = new Map(prev);
      next.set(snapshot.run_id, {
        ...snapshot,
        events: mergeEvents(current?.events ?? [], snapshot.events),
      });
      return next;
    });
  }

  // What is already running: asked once, at open. A run started before a page
  // reload is alive in the shell, and without this question it would keep
  // working while being invisible.
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    knownRuns()
      .then((found) => {
        if (dropped) return;
        for (const snapshot of found) absorb(snapshot);
      })
      // No runs to show is not a failure worth shouting about: the start
      // button still works, and an error here would be about a view nobody
      // has asked for yet.
      .catch(() => {});
    return () => {
      dropped = true;
    };
  }, []);

  // Why the view is not updating itself, when it is not. Empty means the live
  // feed is attached.
  const [listenFailure, setListenFailure] = useState<string | null>(null);

  // The live feed. Attached once and left there: detaching and reattaching on
  // every fact would lose precisely the facts arriving in between.
  useEffect(() => {
    if (!NATIVE) return;
    let unlisten: (() => void) | null = null;
    let dropped = false;
    void listenToRuns((event) => {
      setExecutions((prev) => {
        const current = prev.get(event.run_id);
        if (!current) return prev;
        const events = mergeEvents(current.events, [event]);
        if (events === current.events) return prev;
        const payload = event.payload as { status?: unknown } | null;
        const status =
          event.kind === "run_ended" && typeof payload?.status === "string"
            ? payload.status
            : current.status;
        const next = new Map(prev);
        next.set(event.run_id, { ...current, events, status });
        return next;
      });
    }).then((result) => {
      if ("why" in result) {
        setListenFailure(result.why);
        return;
      }
      if (dropped) result.stop();
      else unlisten = result.stop;
    });
    return () => {
      dropped = true;
      unlisten?.();
    };
  }, []);


  // How each flow is triggered is the shell's answer, not this page's: there
  // is one rule about where a delivery lands, and it lives over there.
  const known = useMemo(() => flowList.map(({ name }) => name).join("\u0000"), [flowList]);
  useEffect(() => {
    if (!NATIVE) return;
    let dropped = false;
    for (const name of known.split("\u0000").filter((entry) => entry !== "")) {
      setTriggers((prev) => {
        if (prev.has(name)) return prev;
        flowTrigger(name)
          .then((trigger) => {
            if (!dropped) setTriggers((now) => new Map(now).set(name, { state: "ready", trigger }));
          })
          .catch((error: unknown) => {
            if (!dropped) setTriggers((now) => new Map(now).set(name, { state: "mute", why: String(error) }));
          });
        return new Map(prev).set(name, { state: "asking" });
      });
    }
    return () => {
      dropped = true;
    };
  }, [known]);

  const anyRunning = useMemo(
    () => Array.from(executions.values()).some((run) => run.status === "running"),
    [executions],
  );

  // The counters' clock. IT ONLY TICKS WHILE SOMETHING RUNS: a fixed one-second
  // redraw of the canvas forever is a cost nobody is watching, and on this
  // canvas one redraw too many is enough to send it into a loop.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    if (!anyRunning) return;
    const tick = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(tick);
  }, [anyRunning]);

  // THE SAFETY NET. An event channel can fail to attach, or drop a fact; either
  // way the window would keep drawing the last state it received — "running for
  // 00:30" on a finished run — which is exactly the lie this view must not
  // tell. While something runs, the state is re-asked and the facts merge by
  // sequence number. It polls **only while a run is alive**, never at rest.
  useEffect(() => {
    if (!NATIVE || !anyRunning) return;
    const live = Array.from(executions.values())
      .filter((run) => run.status === "running")
      .map((run) => run.run_id);
    const tick = window.setInterval(() => {
      for (const runId of live) void runSnapshot(runId).then(absorb).catch(() => {});
    }, 1000);
    return () => window.clearInterval(tick);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [anyRunning, executions.size]);

  // WHAT THE WATCHED RUN IS COSTING, **ASKED OF THE ENGINE AND NOT SUMMED
  // HERE**: `ui::dashboard` computes the totals, the same code that serves
  // `sailor ui`, and a second sum in TypeScript would give two figures for one
  // spend.

  // **EVERY THREE SECONDS WHILE RUNNING, ONCE WHEN IT ENDS.** Reading the spend
  // opens the store and walks the runs: too much for the one-second step tick,
  // too little for a figure that moves only when a step calls an engine. `null`
  // until the store has projected the run, and then nothing is shown rather
  // than a zero that would look like a measurement.
  const [usage, setUsage] = useState<RunUsage | null>(null);
  useEffect(() => {
    if (!NATIVE || !watching) {
      setUsage(null);
      return;
    }
    let alive = true;
    const ask = () => {
      void runUsage(watching)
        .then((found) => {
          if (alive) setUsage(found);
        })
        .catch(() => {});
    };
    ask();
    const watchedRun = executions.get(watching);
    if (watchedRun?.status !== "running") return () => {
      alive = false;
    };
    const tick = window.setInterval(ask, 3000);
    return () => {
      alive = false;
      window.clearInterval(tick);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [watching, executions.get(watching ?? "")?.status, executions.size]);

  /**
   * The watched run's per-step spend, ready for the nodes. Calls carry
   * `step_id` but not the flow: it is added here, by whoever knows which run is
   * watched, because a node's key is `flow::step` and one step id can repeat.
   */
  const stepUsage = useMemo(() => {
    const watchedRun = watching ? executions.get(watching) : undefined;
    if (!watchedRun) return new Map<string, StepUsage>();
    return stepUsageOfRun(usage, watchedRun.flow);
  }, [usage, watching, executions]);

  /** A flow's most recent run: the one the viewer means. */
  const latestByFlow = useMemo(() => {
    const latest = new Map<string, RunSnapshot>();
    for (const run of executions.values()) {
      const current = latest.get(run.flow);
      if (!current || run.started_at >= current.started_at) latest.set(run.flow, run);
    }
    return latest;
  }, [executions]);

  const executionList = useMemo(
    () => Array.from(executions.values()).sort((a, b) => b.started_at - a.started_at),
    [executions],
  );

  async function handleRun(flowName: string) {
    const working = flows.get(flowName);
    if (!working) return;
    // A FLOW RUNS FROM DISK, NOT FROM THE SCREEN. The engine reads the file:
    // with unsaved changes, what starts is not what is on screen — and whoever
    // presses must decide that up front, not learn it from the results.
    if (isDirty(working)) {
      const question =
        working.saved === null
          ? `«${flowName}» non è mai stato salvato: il motore non lo troverebbe sul disco. Salvarlo e poi eseguirlo?`
          : `«${flowName}» ha modifiche non salvate: il motore eseguirebbe la versione sul disco. Salvarlo prima?`;
      if (!window.confirm(question)) return;
      await handleSave(flowName);
      if (saveErrors[flowName]) return;
    }

    setStarting((prev) => new Set(prev).add(flowName));
    setRunErrors((prev) => {
      if (!(flowName in prev)) return prev;
      const next = { ...prev };
      delete next[flowName];
      return next;
    });
    try {
      const text = mandates[flowName] ?? "";
      const started = await startRun(flowName, text.trim() === "" ? null : text);
      absorb({
        run_id: started.run_id,
        flow: started.flow,
        started_at: started.started_at,
        status: "running",
        events: [],
      });
      setWatching(started.run_id);
      // Facts born between the start and the feed attaching live only in the
      // shell: they are asked for at once, and merged by sequence number.
      void runSnapshot(started.run_id)
        .then(absorb)
        .catch((error: unknown) => setRunErrors((prev) => ({ ...prev, [flowName]: String(error) })));
    } catch (error) {
      setRunErrors((prev) => ({ ...prev, [flowName]: String(error) }));
    } finally {
      setStarting((prev) => {
        const next = new Set(prev);
        next.delete(flowName);
        return next;
      });
    }
  }

  const controls: RunControls = useMemo(
    () => ({
      native: NATIVE,
      triggerOf: (name) =>
        triggers.get(name) ??
        (NATIVE
          ? { state: "asking" }
          : { state: "mute", why: "fuori dal guscio: nessun motore da innescare" }),
      runOf: (name) => latestByFlow.get(name),
      mandateOf: (name) => mandates[name] ?? "",
      onMandate: (name, text) => setMandates((prev) => ({ ...prev, [name]: text })),
      onRun: (name) => void handleRun(name),
      starting: (name) => starting.has(name),
      errorOf: (name) => runErrors[name],
      onWatch: (name) => {
        const run = latestByFlow.get(name);
        if (run) setWatching(run.run_id);
      },
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [triggers, latestByFlow, mandates, starting, runErrors, flows, saveErrors],
  );

  const watched = watching ? executions.get(watching) : undefined;

  // SAMPLE STATES STAY WITH THE SAMPLE. Passing `SAMPLE_RUN` here would paint
  // "went" and "broke" on steps nobody ever ran.

  // The empty map is hoisted: a `new Map()` written inline is a fresh object
  // every render, the drawing depends on `runs`, an effect rewrites the nodes,
  // and the render restarts. The canvas never holds still long enough to
  // measure its own nodes — an empty canvas with a full minimap.
  const runs = useMemo(
    () => (source === "sample" ? SAMPLE_RUN : new Map<string, StepRun>()),
    [source],
  );

  // THE REAL NODE STATES, deliberately not routed through the drawing above.
  //
  // `runs` in the drawing means every change rebuilds the node list, which is
  // what this canvas cannot take. The real states go into a context value
  // instead: the nodes stay the same objects and only the readers redraw. Key
  // `flow::step`, because three step ids already repeat across real flows.
  const stepStates = useMemo(
    () => stepStatesOfCanvas(executions.values()),
    [executions],
  );

  const layout = useMemo(() => buildUnifiedLayout(flowList, runs, focusName), [flowList, runs, focusName]);

  /**
   * The triggers are added here and not in `buildUnifiedLayout`, which draws
   * only what the flow file holds: the boundary between engine and window stays
   * visible. Their `data` is two stable strings — run state comes through the
   * context, or each incoming fact would rebuild the list into an endless loop.
   */
  const canvas = useMemo(() => {
    const nodes: Node[] = [...layout.nodes];
    const edges: Edge[] = [...layout.edges];
    const byName = new Map(flowList.map(({ name, flow }) => [name, flow]));

    for (const [name, band] of layout.bands) {
      const id = triggerNodeId(name);
      nodes.push({
        id,
        type: "trigger",
        position: { x: band.x - TRIGGER_WIDTH - TRIGGER_GAP, y: band.y + TRIGGER_TOP },
        draggable: false,
        data: { flowName: name, color: band.color },
      });

      // The trigger points at the steps the graph starts from. Dashed, because
      // it is not a dependency declared in the file: it is the gesture that
      // sets off the ones waiting on nobody.
      const flow = byName.get(name);
      for (const step of flow?.graph.steps ?? []) {
        if (step.deps.length > 0) continue;
        edges.push({
          id: `${id}->${nodeId(name, step.id)}`,
          source: id,
          target: nodeId(name, step.id),
          animated: false,
          selectable: false,
          style: {
            stroke: band.color,
            strokeDasharray: "2 5",
            opacity: focusName !== null && focusName !== name ? 0.2 : 0.7,
          },
        });
      }
    }

    return { nodes, edges };
  }, [layout, flowList, focusName]);

  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  useEffect(() => {
    setNodes(canvas.nodes);
    setEdges(canvas.edges);
  }, [canvas]);

  // Focus moves the viewport, it does not rebuild the layout: the last known
  // box is read from a ref, so a keystroke does not re-centre the canvas while
  // a field is being edited elsewhere.
  const bandsRef = useRef(layout.bands);
  bandsRef.current = layout.bands;
  const flowInstance = useRef<ReactFlowInstance | null>(null);

  useEffect(() => {
    const instance = flowInstance.current;
    if (!instance) return;
    if (focusName === null) {
      instance.fitView({ padding: 0.15, duration: 300 });
      return;
    }
    const band = bandsRef.current.get(focusName);
    if (!band) return;
    void instance.fitBounds(
      { x: band.x, y: band.y, width: band.width, height: band.height },
      { padding: 0.2, duration: 300 },
    );
  }, [focusName]);

  useEffect(() => {
    flowInstance.current?.fitView({ padding: 0.15 });
  }, [source]);

  /**
   * Mounted hidden it measures 0x0, and no other `fitView` fires on first show:
   * hence the observer, once per appearance — refitting on each resize would
   * drag the viewport from under somebody's hands. `minZoom` floors at 0.5
   * against a fit wanting 0.338: a lane stays out, per `unhappystates.test.tsx`.
   */
  const canvasRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (place !== "flows") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const watcher = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box || box.width === 0 || box.height === 0) return;
      watcher.disconnect();
      void flowInstance.current?.fitView({ padding: 0.15 });
    });
    watcher.observe(canvas);
    return () => watcher.disconnect();
  }, [place]);

  /**
   * The other half: a measurable box is not yet a populated one. Nodes arrive a
   * pass later, so a framing that fired earlier framed nothing and never came
   * back — eight nodes, zero in view, from 375 to 1440px. Once only, and the
   * geometry jsdom cannot see is watched by `npm run check:canvas`.
   */
  const framedOnce = useRef(false);
  useEffect(() => {
    if (framedOnce.current || focusName !== null) return;
    const instance = flowInstance.current;
    // Without an instance or a node there is nowhere to go, and the wait ends
    // by itself on the next paint.
    if (!instance || nodes.length === 0) return;
    framedOnce.current = true;
    instance.fitView({ padding: 0.15 });
  }, [nodes, focusName]);

  /**
   * A new step is born parallel to its lane's start — it depends on nobody, and
   * appending it to the chain would invent a dependency nobody asked for. It
   * cannot be born under the pointer: the flow file carries no position, so it
   * would jump on the first recompute. `maxZoom` is pinned, so the view pans.
   */
  const [addedNode, setAddedNode] = useState<string | null>(null);

  useEffect(() => {
    if (addedNode === null) return;
    const instance = flowInstance.current;
    // The node joins the list one pass after the flow changed: until it exists
    // we go nowhere, and the wait ends by itself on the next draw.
    if (!instance || !nodes.some((node) => node.id === addedNode)) return;
    void instance.fitView({
      nodes: [{ id: addedNode }],
      padding: 0.1,
      maxZoom: instance.getZoom(),
      duration: 320,
    });
    setAddedNode(null);
  }, [nodes, addedNode]);

  /**
   * Where a wire was let go, and which step it came from. The gesture the blank
   * canvas has been promising did nothing when the wire landed on empty paper;
   * now it opens a menu and the step is born already wired.
   */
  const [wiring, setWiring] = useState<{ from: string; flowName: string; at: { x: number; y: number } } | null>(null);
  const wireSource = useRef<string | null>(null);

  /** Opens the menu of what could follow a step, from wherever it was asked. */
  const openWireMenu = useCallback((from: string, flowName: string, at: { x: number; y: number }) => {
    setWiring({ from, flowName, at });
  }, []);

  /** Creates a step of `kind` already depending on `from`, in one edit. */
  function addStepAfter(flowName: string, from: string, kind: StepKind) {
    let born: string | null = null;
    updateFlow(flowName, (flow) => {
      const { graph, id } = withStepWiredTo(flow.graph, kind, from);
      born = id;
      return id === null ? flow : { ...flow, graph };
    });
    if (born !== null) {
      setSelectedNode(nodeId(flowName, born));
      setAddedNode(nodeId(flowName, born));
    }
  }

  function updateFlow(name: string, updater: (flow: FlowFile) => FlowFile) {
    setFlows((prev) => {
      const current = prev.get(name);
      if (!current) return prev;
      const next = new Map(prev);
      next.set(name, { ...current, flow: updater(current.flow) });
      return next;
    });
  }

  /**
   * A new flow is born here, empty, and stays in the window until saved:
   * without this gesture an empty folder is a dead end, because the step
   * toolbox needs a flow to belong to.
   */
  function addFlow() {
    const name = freeFlowName(flows);
    const flow: FlowFile = {
      id: name,
      description: "",
      graph: { steps: [], skippable_dependencies: [] },
      inputs: {},
    };
    setFlows((prev) => new Map(prev).set(name, { flow, saved: null, origin: null }));
    setFocusName(name);
    setSelectedNode(null);
  }

  /**
   * Renaming a flow means renaming its file. Before the first write it is
   * harmless; after, the old file would stay on disk beside the new one. The
   * panel allows it only before the first save, and says so.
   */
  function renameFlow(oldName: string, raw: string) {
    const name = raw.trim();
    if (name === "" || name === oldName) return;
    setFlows((prev) => {
      const current = prev.get(oldName);
      if (!current || current.saved !== null || prev.has(name)) return prev;
      const next = new Map<string, WorkingFlow>();
      for (const [key, value] of prev) {
        if (key === oldName) next.set(name, { ...value, flow: { ...value.flow, id: name } });
        else next.set(key, value);
      }
      return next;
    });
    setFocusName((current) => (current === oldName ? name : current));
    setSelectedNode((current) => {
      if (!current) return current;
      const { flowName, stepId } = splitNodeId(current);
      return flowName === oldName ? nodeId(name, stepId) : current;
    });
  }

  function addStep(flowName: string, kind: StepKind) {
    const action = DEFAULT_ACTION_FOR_KIND[kind];
    if (!action) return;
    const working = flows.get(flowName);
    if (!working) return;
    const ids = new Set(working.flow.graph.steps.map((step) => step.id));
    let n = 1;
    let id = `${kind}-${n}`;
    while (ids.has(id)) {
      n += 1;
      id = `${kind}-${n}`;
    }
    const step: Step = {
      id,
      deps: [],
      input_schema: { type: "any" },
      output_schema: { type: "any" },
      with: null,
      when: null,
      action,
      max_attempts: 1,
    };
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: { ...flow.graph, steps: [...flow.graph.steps, step] },
    }));
    setSelectedNode(nodeId(flowName, id));
    setAddedNode(nodeId(flowName, id));
  }

  function deleteStep(flowName: string, stepId: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps
          .filter((step) => step.id !== stepId)
          .map((step) =>
            step.deps.includes(stepId) ? { ...step, deps: step.deps.filter((dep) => dep !== stepId) } : step,
          ),
        skippable_dependencies: (flow.graph.skippable_dependencies ?? []).filter(
          (edge) => edge.step !== stepId && edge.dependency !== stepId,
        ),
      },
    }));
    setSelectedNode((current) => (current === nodeId(flowName, stepId) ? null : current));
  }

  function renameStep(flowName: string, oldId: string, newId: string) {
    updateFlow(flowName, (flow) => {
      if (flow.graph.steps.some((step) => step.id === newId)) return flow;
      return {
        ...flow,
        graph: {
          steps: flow.graph.steps.map((step) => {
            const deps = step.deps.includes(oldId) ? step.deps.map((dep) => (dep === oldId ? newId : dep)) : step.deps;
            if (step.id === oldId) return { ...step, id: newId, deps };
            return deps === step.deps ? step : { ...step, deps };
          }),
          skippable_dependencies: (flow.graph.skippable_dependencies ?? []).map((edge) => ({
            step: edge.step === oldId ? newId : edge.step,
            dependency: edge.dependency === oldId ? newId : edge.dependency,
          })),
        },
      };
    });
    setSelectedNode(nodeId(flowName, newId));
  }

  function updateStepField(flowName: string, stepId: string, patch: Partial<Step>) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === stepId ? { ...step, ...patch } : step)),
      },
    }));
  }

  // Wiring and unwiring two steps: the same gesture, whether it starts as a
  // drag on the canvas or a checkbox in the panel, always comes through here.
  function connectSteps(flowName: string, from: string, to: string) {
    if (from === to) return;
    const working = flows.get(flowName);
    if (!working) return;
    if (wouldCycle(working.flow.graph, from, to)) return;
    const target = working.flow.graph.steps.find((step) => step.id === to);
    if (!target || target.deps.includes(from)) return;
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) => (step.id === to ? { ...step, deps: [...step.deps, from] } : step)),
      },
    }));
  }

  function disconnectSteps(flowName: string, from: string, to: string) {
    updateFlow(flowName, (flow) => ({
      ...flow,
      graph: {
        ...flow.graph,
        steps: flow.graph.steps.map((step) =>
          step.id === to ? { ...step, deps: step.deps.filter((dep) => dep !== from) } : step,
        ),
      },
    }));
  }

  async function handleSave(name: string) {
    const working = flows.get(name);
    if (!working) return;
    setSaving((prev) => new Set(prev).add(name));
    setSaveErrors((prev) => {
      if (!(name in prev)) return prev;
      const next = { ...prev };
      delete next[name];
      return next;
    });
    try {
      const origin = await saveFlow(working.flow);
      setFlows((prev) => {
        const current = prev.get(name);
        if (!current) return prev;
        const next = new Map(prev);
        // The origin arrives from the save and is not kept from before: for a
        // flow born in the window there was none, and the engine decides the
        // place — the project you are looking at, or your home if there is no
        // project. Only it knows which.
        next.set(name, { flow: current.flow, saved: current.flow, origin });
        return next;
      });
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  async function handleDeleteFlow(name: string) {
    const working = flows.get(name);
    if (!working) return;
    // A flow never written is discarded here: asking the engine to delete a
    // file it has never seen would give an error instead of a completed
    // gesture.
    const neverSaved = working.saved === null;
    const question = neverSaved
      ? `Scartare il flusso "${name}"? Non è mai stato scritto sul disco.`
      : `Cancellare il flusso "${name}"? Non si torna indietro da qui.`;
    if (!window.confirm(question)) return;

    function forget() {
      setFlows((prev) => {
        const next = new Map(prev);
        next.delete(name);
        return next;
      });
      setFocusName((current) => (current === name ? null : current));
      setSelectedNode((current) => (current && splitNodeId(current).flowName === name ? null : current));
    }

    if (neverSaved) {
      forget();
      return;
    }

    setSaving((prev) => new Set(prev).add(name));
    try {
      await deleteFlow(name);
      forget();
    } catch (error) {
      setSaveErrors((prev) => ({ ...prev, [name]: String(error) }));
    } finally {
      setSaving((prev) => {
        const next = new Set(prev);
        next.delete(name);
        return next;
      });
    }
  }

  function onNodesChange(changes: NodeChange[]) {
    setNodes((current) => applyNodeChanges(changes, current));
  }

  function onEdgesChange(changes: EdgeChange[]) {
    setEdges((current) => applyEdgeChanges(changes, current));
  }

  function onConnect(connection: Connection) {
    if (!connection.source || !connection.target) return;
    const from = splitNodeId(connection.source);
    const to = splitNodeId(connection.target);
    if (from.flowName !== to.flowName) return;
    connectSteps(from.flowName, from.stepId, to.stepId);
  }

  function isValidConnection(connection: Connection | Edge): boolean {
    const { source, target } = connection;
    if (!source || !target) return false;
    const from = splitNodeId(source);
    const to = splitNodeId(target);
    return from.flowName === to.flowName && from.stepId !== to.stepId;
  }

  function onNodesDelete(deleted: Node[]) {
    for (const node of deleted) {
      if (node.type !== "step") continue;
      const { flowName, stepId } = splitNodeId(node.id);
      deleteStep(flowName, stepId);
    }
  }

  function onEdgesDelete(deleted: Edge[]) {
    for (const edge of deleted) {
      const from = splitNodeId(edge.source);
      const to = splitNodeId(edge.target);
      if (from.flowName !== to.flowName) continue;
      disconnectSteps(to.flowName, from.stepId, to.stepId);
    }
  }

  function onNodeClick(_event: unknown, node: Node) {
    if (node.type !== "step") return;
    setSelectedNode(node.id);
    setFocusName(splitNodeId(node.id).flowName);
  }

  const selected = selectedNode ? nodes.find((node) => node.id === selectedNode) : undefined;
  const selectedData = selected?.data as StepNodeData | undefined;
  const selectedFlow = selectedData ? flows.get(selectedData.flowName) : undefined;

  const focusedBand = focusName ? layout.bands.get(focusName) : undefined;
  const focusedWorking = focusName ? flows.get(focusName) : undefined;
  const focusedDirty = focusedWorking ? isDirty(focusedWorking) : false;

  /**
   * WHAT THE RIGHT-HAND SIDE OF THE BAR IS ALLOWED TO SAY.
   *
   * The mockup writes «one run in progress · step 7 of 9» on the graph, and the
   * verdict of a check on the code. Only the first of the two exists: the count
   * is folded here from the run's own events and the number of steps the flow
   * declares. The second is `sailor flow check`, which lives in the command line
   * and has no door into this window — so the code tab says what the engine
   * really did (it refuses a graph it cannot load, on load and on save) instead
   * of borrowing a verdict nobody gave.
   */
  const focusedRun = focusName ? latestByFlow.get(focusName) : undefined;
  const barStatus = useMemo<BarStatus | null>(() => {
    if (focusName === null || focusedWorking === undefined) return null;
    const total = focusedWorking.flow.graph.steps.length;


    if (focusedRun === undefined) return { live: false, word: "no run of this flow yet" };
    const { done, running } = runProgress(focusedRun);
    if (focusedRun.status === "running") {
      const at = Math.min(total, done + (running > 0 ? 1 : 0));
      return { live: true, word: `a run in progress · step ${at} of ${total}` };
    }
    return { live: false, word: `last run ${focusedRun.status} · ${done} of ${total} steps closed` };
  }, [focusName, focusedWorking, focusedDirty, focusedRun, flowView]);

  return (
    <div className="app">
      {/* THE BAR NAMES THE FLOW IT IS ABOUT.
          Until now it said «6 flows, one system» — true, and about nothing you
          could act on: neither which flow was on screen, nor whether it was
          saved, nor whether it was running, nor a way to run it. Save and Run
          need a subject, and the subject is the flow the rail has in focus. */}
      <TopBar
        view={flowView}
        onView={() => setPlace("flows")}
        onBoard={place === "flows"}
        flowName={focusName}
        steps={focusedWorking ? focusedWorking.flow.graph.steps.length : 0}
        dirty={focusedDirty}
        busy={focusName !== null && saving.has(focusName)}
        starting={focusName !== null && starting.has(focusName)}
        source={source}
        sourceWord={
          source === "engine"
            ? `${flows.size + broken.length} flows from the disk`
            : source === "failed"
              ? `the engine is not answering: ${failure}`
              : source === "loading"
                ? "asking the engine for the flows…"
                : "sample data"
        }
        status={barStatus}
        onWatch={focusedRun ? () => setWatching(focusedRun.run_id) : undefined}
        onSave={() => {
          if (focusName) void handleSave(focusName);
        }}
        onRun={() => {
          if (focusName) void handleRun(focusName);
        }}
      />

      {/* I POSTI, NOMINATI E DIVISI IN DUE INTENZIONI. Una finestra che cambia
          contenuto senza dire dove si è costringe a ricostruirlo dall'aspetto
          della pagina.

          La divisione — guardare a sinistra, amministrare a destra — è il
          criterio che Inngest ha adottato riprogettando la propria
          navigazione, e lo misura in clic: «i run falliti a un clic dalla barra
          laterale» invece che scavando dentro la pagina delle funzioni. È
          l'unico documento della ricognizione che nomina un criterio invece di
          descrivere una disposizione.

          Quelli che mancano — spazi, profili, server MCP — si aggiungono
          quando esiste il motore che li risponde, non prima: una voce che apre
          una schermata vuota è una promessa non mantenuta a ogni clic.

          «Terminali» stava in quell'elenco fino al 01/09/2026 e adesso è una
          voce, e la differenza non è che il motore risponda — il ponte nasce in
          un altro cantiere mentre questa riga si scrive. È che quella schermata
          **non si apre vuota**: senza motore scrive quale domanda non ha potuto
          fare, ed è la distinzione fra «non c'è niente» e «non posso vedere» che
          la promessa non mantenuta faceva sparire. */}
      <nav className="places">
        <button
          type="button"
          className="places__item"
          data-here={place === "now" || undefined}
          onClick={() => setPlace("now")}
        >
          Adesso
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "terminals" || undefined}
          onClick={() => setPlace("terminals")}
        >
          Terminali
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "workspaces" || undefined}
          onClick={() => setPlace("workspaces")}
        >Progetti</button>
        <button
          type="button"
          className="places__item"
          data-here={place === "profiles" || undefined}
          onClick={() => setPlace("profiles")}
        >Profili</button>
        <button
          type="button"
          className="places__item"
          data-here={place === "worktrees" || undefined}
          onClick={() => setPlace("worktrees")}
        >
          Worktrees
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "history" || undefined}
          onClick={() => setPlace("history")}
        >
          Storia
        </button>
        <span className="places__gap" />
        <button
          type="button"
          className="places__item"
          data-here={place === "flows" || undefined}
          onClick={() => setPlace("flows")}
        >
          Flussi
          <span className="places__count">{flows.size}</span>
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "installed" || undefined}
          onClick={() => setPlace("installed")}
        >
          Installato
        </button>
        <button
          type="button"
          className="places__item"
          data-here={place === "manual" || undefined}
          onClick={() => setPlace("manual")}
        >
          Comandi
        </button>
      </nav>

      {place === "flows" && focusName && focusedWorking && focusedBand && (
        <FocusBar
          key={focusName}
          name={focusName}
          color={focusedBand.color}
          flow={focusedWorking.flow}
          neverSaved={focusedWorking.saved === null}
          busy={saving.has(focusName)}
          error={saveErrors[focusName]}
          onRename={(next) => renameFlow(focusName, next)}
          onDescription={(text) => updateFlow(focusName, (flow) => ({ ...flow, description: text }))}
          onDelete={() => void handleDeleteFlow(focusName)}
        />
      )}

      <RunContext.Provider value={controls}>
      <StepRunContext.Provider value={stepStates}>
      <WireContext.Provider value={openWireMenu}>
      <StepUsageContext.Provider value={stepUsage}>
      {place === "now" && (
        <Now
          native={NATIVE}
          onOpen={(runId) => {
            setWatching(runId);
          }}
        />
      )}
      {place === "history" && <History native={NATIVE} />}
      {place === "installed" && <Installed native={NATIVE} />}
      {place === "manual" && <Manual native={NATIVE} />}
      {place === "terminals" && <Terminals native={NATIVE} />}
      {place === "workspaces" && <Projects native={NATIVE} now={now} />}
      {place === "profiles" && <ProfileList native={NATIVE} />}
      {place === "worktrees" && <Worktrees native={NATIVE} />}

      {/* Outside the canvas element on purpose: it is positioned in window
          coordinates, and inside it would scroll and scale with the paper. */}
      {wiring && (
        <WireMenu
          at={wiring.at}
          from={wiring.from}
          onPick={(kind) => {
            addStepAfter(wiring.flowName, wiring.from, kind);
            setWiring(null);
          }}
          onClose={() => setWiring(null)}
        />
      )}
      {/* THE CANVAS STAYS MOUNTED BEHIND THE OTHER TWO TABS. React Flow measures
          its own frame once: unmounting it to change tab would give back a
          canvas that has to find its nodes again every time. */}
      <div className="body" hidden={place !== "flows" || flowView !== "graph"}>

        {/* LA COLONNA HA UN MESTIERE SOLO: SCEGLIERE COSA GUARDARE. La cassetta
            dei passi se n'è andata dentro la tela, dove si compone. Quello che
            resta — l'elenco dei flussi, «tutti i flussi», il flusso nuovo —
            risponde a una domanda sola, e la larghezza resta quella: a fissarla
            era il nome di un flusso accanto al suo conteggio di passi, e
            stringere la colonna li troncherebbe senza guadagnare niente sulla
            tela, che il pannello a destra delimita comunque. */}
        {/* A ZERO FLUSSI LA COLONNA SI CHIUDE, come la destra. Un elenco di
            niente col suo invito accanto metteva DUE INVITI ALLO STESSO GESTO:
            «+ Nuovo flusso» qui e «Crea il primo flusso» sulla tela, la stessa
            funzione con due nomi a mezzo metro. E il gesto non perde la sua
            casa, perché i due non convivono mai: al primo clic la scheda se ne
            va e la colonna torna, col bottone accanto alla cosa appena creata. */}
        {(flows.size > 0 || broken.length > 0) && (
        <aside className="rail">
          <div className="rail__title">Flussi registrati</div>
          {/* «Tutti i flussi» toglie il fuoco: senza flussi non c'è fuoco da
              togliere, e il bottone resterebbe un comando che non comanda. */}
          {flows.size > 0 && (
          <button
            type="button"
            className="rail__all"
            data-active={focusName === null || undefined}
            onClick={() => setFocusName(null)}
          >
            Tutti i flussi
          </button>
          )}
          {/* A FLAT LIST SAID WHOSE NOTHING WAS. Flows arrive from three
              places, and mixed together a flow of the project you are in sat
              between two of somebody else's with no way to tell. The heading
              also answers what came next: two flows can share a name, and the
              most specific place wins. */}
          {railGroups.map((group) => (
            <div className="rail__group" key={group.origin ?? UNSAVED_GROUP}>
              <div className="rail__origin">{group.origin ?? UNSAVED_GROUP}</div>
              {group.flows.map(({ name, flow }) => {
                const working = flows.get(name);
                const dirty = working ? isDirty(working) : false;
                const color = layout.bands.get(name)?.color;
                return (
                  <button
                    type="button"
                    key={name}
                    className="rail__item"
                    data-open={name === focusName || undefined}
                    onClick={() => {
                      setFocusName((current) => (current === name ? null : name));
                      setSelectedNode(null);
                    }}
                  >
                    <span className="rail__dot" style={{ background: color }} />
                    <span className="rail__label">
                      {name}
                      {dirty && <span className="rail__dirty-dot" title="non salvato" />}
                    </span>
                    <span className="rail__note">{stepCountLabel(flow.graph.steps.length)}</span>
                  </button>
                );
              })}
              {group.broken.map((entry) => (
                // A broken flow does not vanish from the list: it is shown,
                // marked, with the reason. It stays off the canvas because it
                // has no graph to draw — and it stays under its origin, which
                // is where whoever goes to repair it has to look.
                <div className="rail__item" key={entry.name} data-broken>
                  <span className="rail__label">{entry.name}</span>
                  <span className="rail__note">{entry.reason}</span>
                </div>
              ))}
            </div>
          ))}
          {/* L'INVITO È DELLA TELA FINCHÉ NON C'È NIENTE. Coi soli flussi rotti
              la colonna resta aperta — sparire porterebbe via il posto che la
              scheda nomina — ma tace: il gesto lo offre la scheda, da sola. */}
          {flows.size > 0 && (
          <button type="button" className="rail__new" onClick={addFlow}>
            + Nuovo flusso
          </button>
          )}
        </aside>
        )}

        <main className="canvas" ref={canvasRef}>
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            isValidConnection={isValidConnection}
            onNodesDelete={onNodesDelete}
            onEdgesDelete={onEdgesDelete}
            deleteKeyCode={["Backspace", "Delete"]}
            onNodeClick={onNodeClick}
            onPaneClick={() => {
              setSelectedNode(null);
              setWiring(null);
            }}
            onConnectStart={(_, { nodeId: source }) => {
              wireSource.current = source ?? null;
            }}
            onConnectEnd={(event) => {
              const source = wireSource.current;
              wireSource.current = null;
              if (!source) return;
              // A WIRE LET GO ON EMPTY PAPER ASKS A QUESTION. Released over a
              // node, `onConnect` has already made the edge and there is
              // nothing to ask; released over nothing, the menu opens here.
              const target = event.target as HTMLElement | null;
              if (!target?.classList.contains("react-flow__pane")) return;
              const { flowName, stepId } = splitNodeId(source);
              const point = "clientX" in event ? event : event.changedTouches[0];
              setWiring({ from: stepId, flowName, at: { x: point.clientX, y: point.clientY } });
            }}
            onInit={(instance) => {
              flowInstance.current = instance;
            }}
            fitView
            proOptions={{ hideAttribution: false }}
          >
            {/* GRAPH PAPER AT TWO PITCHES, not generic dots: the fine one
                gives the eye a measure, the coarse one gives it a place. Both
                are declared under 1.5:1, because a grid that reads competes
                with the nodes and the nodes are the figure. */}
            <Background id="fine" gap={12} variant={BackgroundVariant.Lines} color="var(--grid-fine)" />
            <Background id="coarse" gap={96} variant={BackgroundVariant.Lines} color="var(--grid-coarse)" />

            {/* I COMANDI COMANDANO QUALCOSA, o non ci sono. Quattro bottoni che
                ingrandiscono e inquadrano il nulla sono lo stesso «riquadro che
                si vede e non dice niente» per cui qui sotto sparisce la
                minimappa: il criterio è uno, e vale per tutti e due.

                La carta a quadretti resta: è la tela, non un comando, ed è
                quello che fa leggere lo spazio sotto il riquadro come una
                superficie da riempire. Resta anche la firma di React Flow, che
                è una nota di licenza — `hideAttribution` è un'opzione a
                pagamento, e toglierla è una cosa che si compra, non una scelta
                di schermata. */}
            {flows.size > 0 && <Controls />}

            {/* LA MINIMAPPA DICE DOVE GUARDARE, non «c'è della roba». Era un
                blocco grigio uniforme: adesso ogni passo ci sta con la tinta
                del proprio stato, così un guasto in fondo a un flusso fuori
                schermo si vede senza scorrere.

                CON ZERO FLUSSI NON C'È, per la stessa ragione della cassetta:
                una mappa di niente è un riquadro che si vede e non dice
                niente, e sullo schermo che insegna il primo gesto ogni cosa
                muta è una distrazione. È anche la mitigazione del limite
                dichiarato in `unhappystates.test.tsx` — dove non c'è niente da
                mitigare, non serve. */}
            {flows.size > 0 && (
              <MiniMap
                pannable
                zoomable
                nodeColor={(node) => {
                  if (node.type !== "step") return "#d8d7ce";
                  const data = node.data as StepNodeData;
                  const state = stepStates.get(nodeId(data.flowName, data.step.id))?.state ?? data.run?.state;
                  return STATE_COLOR[state ?? "waiting"];
                }}
              />
            )}

            {/* LA CASSETTA STA QUI DENTRO, ed è tutto il punto del lavoro: chi
                compone non esce più dalla tela per prendere un attrezzo.
                Sta dentro `ReactFlow` e non accanto perché `Panel` la disegna
                fuori dal riquadro che pan e zoom trasformano — dentro la tela,
                ferma rispetto ad essa.

                CON ZERO FLUSSI NON C'È: quel momento è della tela vuota, che
                insegna il primo gesto. Due inviti nello stesso schermo si
                annullano — è la stessa regola per cui una vista ha un solo
                elemento isolato. */}
            {flows.size > 0 && (
              <Toolbar
                flowName={focusName}
                onAdd={(kind) => focusName && addStep(focusName, kind)}
              />
            )}
          </ReactFlow>

          {/* Una tela senza flussi non resta muta: dice cos'è un flusso e offre
              il gesto per farne uno. Il messaggio sparisce da solo appena ce n'è
              uno, e non intercetta il puntatore sulla tela sotto. */}
          {flows.size === 0 && (
            <BlankCanvas
              state={source === "failed" ? "failed" : source === "loading" ? "loading" : "empty"}
              failure={failure}
              brokenCount={broken.length}
              onCreate={addFlow}
            />
          )}
        </main>

        {/* LA COLONNA DESTRA SI CHIUDE A ZERO FLUSSI, e la tela si prende la sua
            larghezza. Far tacere il contenuto lasciando in piedi il contenitore
            lasciava una striscia di 288px muta e divisa da un filo: non si legge
            come calma, si legge come una parte di schermo che non ha finito di
            caricare.

            È la regola già applicata tre volte in questo schermo — la barra
            sparisce, la minimappa sparisce, i comandi spariscono: dove non c'è
            niente da mostrare non serve un posto dove mostrarlo. */}
        {flows.size > 0 && (
          <aside className="panel">
            <datalist id="known-actions">
              {KNOWN_ACTIONS.map((action) => (
                <option key={action} value={action} />
              ))}
            </datalist>

            {selectedData && selectedFlow ? (
              <>
                {/* The step as the run has it, above the step as the file
                    declares it: what is happening explains what is written,
                    not the other way round. It mounts only while a run holds
                    this step. */}
                {watched && (
                  <StepLive
                    key={`${selectedData.flowName}::${selectedData.step.id}::live`}
                    step={selectedData.step}
                    graph={selectedFlow.flow.graph}
                    run={watched}
                    now={now}
                  />
                )}
                <StepEditor
                  key={selectedNode}
                  flowName={selectedData.flowName}
                  color={selectedData.color}
                  step={selectedData.step}
                  siblingIds={selectedFlow.flow.graph.steps.map((step) => step.id).filter((id) => id !== selectedData.step.id)}
                  tools={tools}
                  discovery={discovery}
                  usedModels={usedModels}
                  onRename={(newId) => renameStep(selectedData.flowName, selectedData.step.id, newId)}
                  onField={(patch) => updateStepField(selectedData.flowName, selectedData.step.id, patch)}
                  onToggleDep={(depId, on) =>
                    on
                      ? connectSteps(selectedData.flowName, depId, selectedData.step.id)
                      : disconnectSteps(selectedData.flowName, depId, selectedData.step.id)
                  }
                  onDelete={() => deleteStep(selectedData.flowName, selectedData.step.id)}
                />
                {/* Cosa è passato di qui, nel tempo. Sta sotto i parametri e non
                    in un pannello a parte: chi clicca un nodo chiede tutte e due
                    le cose — com'è fatto, e cosa ci è entrato. Fuori dal guscio
                    non c'è deposito, e il pannello lo dice da sé. */}
                {NATIVE && (
                  <StepHistory
                    key={`${selectedData.flowName}::${selectedData.step.id}`}
                    flowName={selectedData.flowName}
                    stepId={selectedData.step.id}
                  />
                )}
              </>
            ) : (
              /* THE PANEL TALKS ABOUT A STEP, the only thing it shows. Asking
                 for a flow too puts TWO INVITATIONS ON ONE SCREEN, and with a
                 flow already focused it invites what is already done. At zero
                 flows there is no rail at all, so promising that step
                 parameters appear here would promise the impossible. */
              <div className="panel__empty">
                {focusName === null
                  ? "I parametri di un passo compaiono qui."
                  : "Scegli un passo sulla tela per vederne e modificarne i parametri."}
              </div>
            )}
          </aside>
        )}
      </div>

      {/* LA VISTA SI CHIUDE, LA CORSA NO. Chiudere questo pannello non ferma
          niente: il flusso gira nel guscio, e riaprendo si ritrova tutto quello
          che ha detto mentre nessuno guardava. */}
      {watched && (
        <RunConsole
          run={watched}
          runs={executionList}
          mode={consoleMode}
          now={now}
          listenFailure={listenFailure}
          usage={usage}
          onMode={setConsoleMode}
          onPick={setWatching}
          onClose={() => setWatching(null)}
        />
      )}
      </StepUsageContext.Provider>
      </WireContext.Provider>
      </StepRunContext.Provider>
      </RunContext.Provider>
    </div>
  );
}

/** A stable empty list: a fresh `[]` each render would redo the work downstream. */
const EMPTY_TOOLS: Tool[] = [];

interface FocusBarProps {
  name: string;
  color: string;
  flow: FlowFile;
  neverSaved: boolean;
  busy: boolean;
  error?: string;
  onRename: (next: string) => void;
  onDescription: (text: string) => void;
  onDelete: () => void;
}

/**
 * The focused flow's bar: name, description and delete. Save left it when the
 * top bar took it — two Save buttons a hand apart ask which one saves what. The
 * name is editable only before the first save, since it is the filename; drafts
 * settle on `blur`, or a rename would fire once per letter typed.
 */
function FocusBar({
  name,
  color,
  flow,
  neverSaved,
  busy,
  error,
  onRename,
  onDescription,
  onDelete,
}: FocusBarProps) {
  const [nameDraft, setNameDraft] = useState(name);
  const [descDraft, setDescDraft] = useState(flow.description);

  return (
    <div className="focusbar">
      <span className="focusbar__dot" style={{ background: color }} />
      {neverSaved ? (
        <input
          className="focusbar__name-input"
          value={nameDraft}
          aria-label="nome del flusso"
          onChange={(event) => setNameDraft(event.target.value)}
          onBlur={() => onRename(nameDraft)}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      ) : (
        <span className="focusbar__name" title="il nome è quello del file: si sceglie prima di salvare">
          {name}
        </span>
      )}
      <input
        className="focusbar__desc-input"
        value={descDraft}
        aria-label="descrizione del flusso"
        placeholder="a cosa serve questo flusso"
        onChange={(event) => setDescDraft(event.target.value)}
        onBlur={() => onDescription(descDraft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
      <div className="focusbar__spacer" />
      {error && <span className="focusbar__error">{error}</span>}
      <button type="button" className="is-danger" onClick={onDelete} disabled={busy}>
        {neverSaved ? "Scarta flusso" : "Elimina flusso"}
      </button>
    </div>
  );
}


// ── the top bar, and the three views of one flow ─────────────────────────

/** What the right-hand side of the bar says, and whether something is alive. */
interface BarStatus {
  live: boolean;
  word: string;
}

/**
 * How far a run has got, folded from its own facts.
 *
 * THE SNAPSHOT CARRIES NO COUNTERS. `RunSnapshot` has `status` and a list of
 * events and nothing else; the ledger has `steps_went`/`steps_total`, but it
 * only counts steps it has already recorded, so mid-run it undercounts, and it
 * is read on demand rather than at every beat. The denominator therefore comes
 * from the flow on screen, and the numerator from the events — the same fold
 * the canvas already uses to colour its nodes.
 */
function runProgress(run: RunSnapshot): { done: number; running: number } {
  let done = 0;
  let running = 0;
  for (const step of stepStatesOfRun(run.events).values()) {
    if (step.state === "running") running += 1;
    else if (step.state !== "waiting") done += 1;
  }
  return { done, running };
}

const VIEW_WORD: Record<FlowView, string> = { graph: "Graph" };

interface TopBarProps {
  view: FlowView;
  onView: (view: FlowView) => void;
  /**
   * Whether the board — and with it the column — is the place in view. The bar
   * is the program's and is drawn everywhere; the line about focusing a flow
   * belongs to one place only.
   */
  onBoard: boolean;
  flowName: string | null;
  steps: number;
  dirty: boolean;
  busy: boolean;
  starting: boolean;
  source: Source;
  sourceWord: string;
  status: BarStatus | null;
  onWatch?: () => void;
  onSave: () => void;
  onRun: () => void;
}

/**
 * The bar of the program: the mark, the flow in focus, the three views of it,
 * and the two gestures that act on it.
 *
 * NO VERSION SITS NEXT TO THE NAME. The mockup draws a `v7` chip there; a flow
 * has no version — not in `flow::FlowFile`, not in the `.flow.json` on disk,
 * not in this window — and the Rust type refuses unknown fields, so one cannot
 * be added without changing the engine. The chip counts steps instead, which is
 * a number the flow really carries.
 */
function TopBar({
  view,
  onView,
  onBoard,
  flowName,
  steps,
  dirty,
  busy,
  starting,
  source,
  sourceWord,
  status,
  onWatch,
  onSave,
  onRun,
}: TopBarProps) {
  const statusBody = status && (
    <>
      <span className="topbar__live" data-idle={status.live ? undefined : true} />
      <span className="topbar__status-word">{status.word}</span>
    </>
  );

  return (
    <header className="topbar">
      <span className="topbar__brand">
        <svg
          className="topbar__mark"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 17l9-13 9 13" />
          <path d="M3 17c2.5 2 5 2 7.5 0S16 15 18.5 17" />
        </svg>
        Sailor
      </span>
      <span className="topbar__rule" />

      {/* A LINE THAT NAMES THE COLUMN IS SILENT WHERE THERE IS NO COLUMN. The
          window opens away from the board, which is the only place holding
          one, and six places out of seven were being sent somewhere they
          are not. */}
      {flowName === null ? (
        onBoard && <span className="topbar__none">no flow in focus — pick one in the rail</span>
      ) : (
        <span className="topbar__flow">
          <span className="topbar__flow-name">{flowName}</span>
          <span className="topbar__steps">{steps} steps</span>
          {dirty && (
            <span className="topbar__dirty">
              <span className="topbar__dot" />
              unsaved changes
            </span>
          )}
        </span>
      )}

      {/* THREE VIEWS OF THE SAME FLOW, not three places of the window. */}
      <nav className="topbar__tabs" aria-label="views of the flow in focus">
        {(["graph"] as FlowView[]).map((name) => (
          <button
            key={name}
            type="button"
            className="topbar__tab"
            data-here={view === name || undefined}
            aria-pressed={view === name}
            onClick={() => onView(name)}
          >
            {VIEW_WORD[name]}
          </button>
        ))}
      </nav>

      <span className="topbar__spacer" />

      {/* Whoever is looking must know whether these flows come from the disk or
          from a sample, without asking and without opening the code. */}
      <span className="topbar__source" data-source={source}>
        {sourceWord}
      </span>

      {status !== null &&
        (onWatch ? (
          <button type="button" className="topbar__status" onClick={onWatch}>
            {statusBody}
          </button>
        ) : (
          <span className="topbar__status">{statusBody}</span>
        ))}

      <button
        type="button"
        className="topbar__save"
        onClick={onSave}
        disabled={flowName === null || !dirty || busy}
      >
        {busy ? "Saving…" : "Save"}
      </button>
      {/* A FILLED BUTTON HAS NO TINT. Prohibition 4 reserves colour for the
          state of the machine, so «Run» is a plate — ink on ground — and not a
          green. `.is-primary` already carried that answer. */}
      <button
        type="button"
        className="topbar__run is-primary"
        onClick={onRun}
        disabled={flowName === null || starting}
      >
        <span className="topbar__glyph" aria-hidden="true">
          ▶
        </span>
        {starting ? "Starting…" : "Run"}
      </button>
    </header>
  );
}

/**
 * THE CODE TAB SHOWS WHAT IS ACTUALLY WRITTEN, which is JSON.
 *
 * The mockup draws TypeScript — `createStep`, `createFlow`, imports from
 * `@sailor/core`. Nothing of the sort exists: there is no package, no generator,
 * no engine command that emits source, and the project decided the opposite on
 * purpose — a flow is a data file and a step is an action registered in Rust,
 * with no interpreter inside Sailor. So this tab does not draw an imaginary
 * language: it shows the file the engine reads, and says why it is that file.
 */
/* Kept, unmounted: what it measured is worth more than the view — there is no
   Tauri command for `sailor flow check`, so the window only knows the graph
   loaded. See the top bar's status line. */
function FlowCode({ name, flow }: { name: string | null; flow: FlowFile | undefined }) {
  if (name === null || flow === undefined) {
    return (
      <div className="codeview">
        <p className="codeview__empty">Pick a flow in the rail to read what is written for it.</p>
      </div>
    );
  }

  return (
    <div className="codeview">
      <div className="label">{name}.flow.json</div>
      <p className="codeview__note">
        This is the flow as it is stored, and it is the whole of it: a flow is a data file, and every
        step names an action the engine has registered — never code that runs from here. The mockup
        for this tab drew generated TypeScript; no such generator exists, and none is planned, so
        what you read below is the file and not a rendering of it.
      </p>
      <pre className="codeview__source">{JSON.stringify(flow, null, 2)}</pre>
    </div>
  );
}

/**
 * THE RUNS TAB IS THE LEDGER, NARROWED BY HAND.
 *
 * The engine keeps every run in a ledger and hands the window the lot: there is
 * no per-flow query behind `execution_history`, though the SQL for one already
 * exists unexposed in `crates/ledger`. Filtering here is what the command line
 * does too — but it means the window carries the whole history to show a slice
 * of it, and that is worth knowing before this list gets long.
 */
/* Kept, unmounted: the ledger has flow-scoped queries (`runs_in_window`,
   `last_finished_run`) that no Tauri command exposes, so this filtered in the
   window. The places «Adesso» and «Cronologia» serve the need. */
function FlowRuns({ name }: { name: string | null }) {
  const now = useClock();
  const { asked } = useAsk<Execution[]>(
    NATIVE,
    executionHistory,
    15000,
    "outside the shell: the ledger is the engine's to read",
  );

  if (name === null) {
    return (
      <div className="now">
        <p className="now__empty">Pick a flow in the rail to see the runs it has had.</p>
      </div>
    );
  }
  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">I cannot read the runs of «{name}»: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Reading the ledger…</p>
      </div>
    );
  }

  const mine = asked.value.filter((run) => run.entity === name);
  if (mine.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">
          The ledger remembers no run of «{name}». It remembers {asked.value.length} of other flows.
        </p>
      </div>
    );
  }

  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Runs of {name}</h2>
        <span className="now__count">{mine.length}</span>
        <span className="now__note">out of {asked.value.length} the ledger remembers</span>
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>run</th>
            <th>how it ended</th>
            <th>when</th>
            <th className="now__num">lasted</th>
            <th className="now__num">steps</th>
            <th className="now__num">retried</th>
          </tr>
        </thead>
        <tbody>
          {mine.map((run) => (
            <tr key={run.run_id}>
              <td className="now__entity">
                {run.run_id}
                {run.error !== null && <span className="now__why">{run.error}</span>}
              </td>
              <td className="now__state" data-outcome={outcomeOf(run)}>
                {outcomeOf(run)}
              </td>
              <td className="now__when">{whenOf(run.started_at, now)}</td>
              <td className="now__num">{lastedOf(run.duration_secs)}</td>
              <td className="now__num">
                {run.steps_went}/{run.steps_total}
              </td>
              <td className="now__num">{run.steps_retried === 0 ? "—" : run.steps_retried}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* Not mounted today, and not dead: see the notes above each one. */
void FlowCode;
void FlowRuns;
