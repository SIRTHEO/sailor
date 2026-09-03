import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
import { BlankCanvas, type PlacesAsk } from "./BlankCanvas";
import { MACHINE, MACHINE_GROUND, PLACES, inTheStrip, type Section } from "./places";
import { World, type FlowGroup } from "./World";
import { ChangesScreen } from "./ChangesScreen";
import type { Project } from "./workspaces";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { TerminalSummary } from "./terminal";
import { BuildChip, LiveChip, WhoChip } from "./Bar";
import { LedgerBrowser } from "./LedgerBrowser";
import { Memory, MEMORY_TABS, type MemoryTab } from "./Memory";
import { SailorScreen } from "./SailorScreen";
import { type SailorTab } from "./sailortabs";
import { TerminalsSection, TERMINALS_TABS, type TerminalsTab } from "./TerminalsSection";
import { declaredCeiling } from "./terminal";
import { Palette, isPaletteKey, type Entry } from "./Palette";
import { StepEditor } from "./StepEditor";
import { StepLive } from "./StepLive";
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
  flowPlaces,
  flowTrigger,
  insideTheWindow,
  knownRuns,
  listenToRuns,
  loadFlows,
  runSnapshot,
  runUsage,
  saveFlow,
  startRun,
  stopRun,
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
type Place = Section;

/* Only the graph. "Code" was a data file dressed as source and "Runs" is
   already the «Adesso» and «Cronologia» places. What the two unmounted screens
   had measured about the engine is in `docs/guasti-incontrati.md`. */

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

/** The first ceiling any loaded flow declares for measuring a terminal, or `null`. */
function ceilingOf(flows: Map<string, WorkingFlow>): number | null {
  for (const working of flows.values()) {
    const ceiling = declaredCeiling(working.flow.graph.steps);
    if (ceiling !== null) return ceiling;
  }
  return null;
}

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
  while (taken.has(`flow-${n}`)) n += 1;
  return `flow-${n}`;
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

  const [place, setPlace] = useState<Place>("board");
  const [memoryTab, setMemoryTab] = useState<MemoryTab>("runs");
  const [sailorTab, setSailorTab] = useState<SailorTab>("keeps");
  /** The tree the window stands in, as the column reads it. */
  const [standingIn, setStandingIn] = useState<Project | null>(null);
  const [terminalsTab, setTerminalsTab] = useState<TerminalsTab>("live");
  const [terminalCount, setTerminalCount] = useState(0);
  /** The terminals themselves: the column nests each under the tree it runs in. */
  const [openTerminals, setOpenTerminals] = useState<TerminalSummary[]>([]);
  const [ledgerTable, setLedgerTable] = useState<string | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (isPaletteKey(event)) {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Focus belongs to the branch, not the canvas: the rail points at a path
  // inside the single graph, it does not choose which graph to show.
  const [focusName, setFocusName] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);

  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [saveErrors, setSaveErrors] = useState<Record<string, string>>({});

  const [discovery, setDiscovery] = useState<ToolDiscovery>(() =>
    NATIVE ? { state: "asking" } : { state: "mute", why: "outside the shell: the engine knows the tools" },
  );

  // The flows are read from the disk at the start and again whenever the
  // window moves into another project: `still` says whether the reader is
  // still on screen when the answer arrives.
  const readFlows = useCallback((still: () => boolean) => {
    if (!NATIVE) return;
    loadFlows()
      .then((loaded) => {
        if (!still()) return;
        const split = splitEntries(loaded);
        setFlows(split.flows);
        setBroken(split.broken);
        setSource("engine");
        setFocusName(null);
        setSelectedNode(null);
      })
      .catch((error: unknown) => {
        if (!still()) return;
        // A silent engine is not papered over with the sample: that would show
        // flows that are not on disk.
        setSource("failed");
        setFailure(String(error));
      });
  }, []);

  useEffect(() => {
    let dropped = false;
    readFlows(() => !dropped);
    return () => {
      dropped = true;
    };
  }, [readFlows]);

  // WHERE THE ENGINE LOOKED, asked only when it found nothing: an empty board
  // that cannot say where it looked cannot be told from a fault.
  const [places, setPlaces] = useState<PlacesAsk>(null);
  const boardEmpty = source === "engine" && flows.size === 0;
  useEffect(() => {
    if (!NATIVE || !boardEmpty) {
      setPlaces(null);
      return;
    }
    let dropped = false;
    flowPlaces().then(
      (found) => {
        if (!dropped) setPlaces({ state: "ready", places: found });
      },
      (error: unknown) => {
        if (!dropped) setPlaces({ state: "mute", why: String(error) });
      },
    );
    return () => {
      dropped = true;
    };
  }, [boardEmpty]);

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
          ? `«${flowName}» has never been saved: the engine would not find it on the disk. Save it, then run it?`
          : `«${flowName}» has unsaved changes: the engine would run the version on the disk. Save it first?`;
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
          : { state: "mute", why: "outside the shell: no engine to trigger" }),
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
    if (place !== "board") return;
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
      ? `Discard the flow "${name}"? It has never been written to the disk.`
      : `Delete the flow "${name}"? There is no way back from here.`;
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

  // The column's flows in the shape the world column draws. The wording and
  // the lane colour stay here, with the flows.
  const worldGroups = useMemo<FlowGroup[]>(
    () =>
      railGroups.map((group) => ({
        origin: group.origin,
        flows: group.flows.map(({ name, flow }) => {
          const working = flows.get(name);
          return {
            name,
            note: stepCountLabel(flow.graph.steps.length),
            color: layout.bands.get(name)?.color,
            dirty: working ? isDirty(working) : false,
          };
        }),
        broken: group.broken.map((entry) => ({ name: entry.name, reason: entry.reason })),
      })),
    [railGroups, layout, flows],
  );

  const focusedBand = focusName ? layout.bands.get(focusName) : undefined;
  const focusedWorking = focusName ? flows.get(focusName) : undefined;
  const focusedDirty = focusedWorking ? isDirty(focusedWorking) : false;

  /**
   * WHAT THE RIGHT-HAND SIDE OF THE BAR MAY SAY. The count is folded from the
   * run's own events and the steps the flow declares. The verdict of a check
   * is not: `sailor flow check` has no door into this window, and borrowing a
   * verdict nobody gave is worse than not showing one.
   */
  const focusedRun = focusName ? latestByFlow.get(focusName) : undefined;
  // WHERE YOU ARE, IN WORDS: the section and the entry inside it. A window
  // that changes content without saying where it is makes the person read it
  // off the shape of the page.
  const crumbs = useMemo<string[]>(() => {
    const section = PLACES.find((one) => one.id === place)?.name ?? place;
    if (place === "board") return focusName === null ? [section] : [section, focusName];
    if (place === "memory") {
      const tab = MEMORY_TABS.find((one) => one.id === memoryTab)?.name ?? memoryTab;
      return [section, tab];
    }
    // «Sailor» is not a place a person picks any more: what used to hide under
    // it is a ground, and the bar says the ground and then the row.
    if (place === "ledger") {
      const row = MACHINE.find((one) => one.section === "ledger");
      const named = [MACHINE_GROUND, row?.name ?? section];
      return ledgerTable === null ? named : [...named, ledgerTable];
    }
    if (place === "sailor") {
      const row = MACHINE.find((one) => one.tab === sailorTab);
      return [MACHINE_GROUND, row?.name ?? sailorTab];
    }
    return [section, TERMINALS_TABS.find((one) => one.id === terminalsTab)?.name ?? terminalsTab];
  }, [place, focusName, memoryTab, sailorTab, terminalsTab, ledgerTable]);

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
  }, [focusName, focusedWorking, focusedDirty, focusedRun]);

  // WHAT ⌘K CAN REACH: every place and entry, every flow to open or to run.
  // The palette computes nothing; the gestures are the same the rail and the
  // bar own, handed in by name.
  const paletteEntries = useMemo<Entry[]>(() => {
    // The same words the column uses. It said «Sailor › Profiles» while the
    // column said «Profiles», so searching for what you can see found nothing.
    const go: Entry[] = inTheStrip().map((one) => ({
      group: "Go to",
      label: one.name,
      hint: one.asks,
      run: () => setPlace(one.id),
    }));
    go.push({
      group: "Go to",
      label: "Board",
      hint: PLACES[0].asks,
      run: () => setPlace("board"),
    });
    for (const one of MACHINE) {
      go.push({
        group: "Go to",
        label: one.name,
        hint: one.asks,
        run: () => {
          if (one.tab !== undefined) setSailorTab(one.tab);
          setPlace(one.section);
        },
      });
    }
    for (const one of MEMORY_TABS) {
      go.push({ group: "Go to", label: `Memory › ${one.name}`, hint: one.about, run: () => { setPlace("memory"); setMemoryTab(one.id); } });
    }
    for (const one of TERMINALS_TABS) {
      go.push({ group: "Go to", label: `Terminals › ${one.name}`, hint: one.about, run: () => { setPlace("terminals"); setTerminalsTab(one.id); } });
    }
    const names = Array.from(flows.keys()).sort();
    const open: Entry[] = names.map((name) => ({
      group: "Open flow",
      label: name,
      hint: flows.get(name)?.origin ?? undefined,
      run: () => {
        setPlace("board");
        setFocusName(name);
      },
    }));
    const run: Entry[] = names.map((name) => ({
      group: "Run flow",
      label: name,
      hint: flows.get(name)?.origin ?? undefined,
      run: () => void handleRun(name),
    }));
    return [...go, ...open, ...run];
  }, [flows, handleRun]);

  return (
    <TooltipProvider>
    <div className="app">
      <Palette entries={paletteEntries} open={paletteOpen} onClose={() => setPaletteOpen(false)} />
      {/* THE BAR NAMES THE FLOW IT IS ABOUT.
          Until now it said «6 flows, one system» — true, and about nothing you
          could act on: neither which flow was on screen, nor whether it was
          saved, nor whether it was running, nor a way to run it. Save and Run
          need a subject, and the subject is the flow the rail has in focus. */}
      <TopBar
        onBoard={place === "board"}
        crumbs={crumbs}
        chips={
          <>
            <button type="button" className="topbar__palette" onClick={() => setPaletteOpen(true)}>
              <kbd className="topbar__kbd">⌘K</kbd>
              Search or run a command
            </button>
            <LiveChip
              native={NATIVE}
              now={now}
              onOpen={(runId) => setWatching(runId)}
              onSpend={() => {
                setPlace("memory");
                setMemoryTab("spend");
              }}
            />
            <BuildChip native={NATIVE} now={now} />
            <WhoChip native={NATIVE} />
          </>
        }
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

      {/* The places, in a column, divided by what you are doing. */}
      <div className="app__body">
      <World
        native={NATIVE}
        here={place}
        hereTab={sailorTab}
        onGo={setPlace}
        onOpen={(section, tab) => {
          if (tab !== undefined) setSailorTab(tab);
          setPlace(section);
        }}
        counts={{ board: flows.size, terminals: terminalCount }}
        terminals={openTerminals}
        onMoved={() => readFlows(() => true)}
        onTree={setStandingIn}
        flowGroups={worldGroups}
        focusName={focusName}
        onFlow={(name) => {
          setFocusName((current) => (name !== null && current === name ? null : name));
          setSelectedNode(null);
        }}
        onNewFlow={addFlow}
      />
      <div className="stage">

      {place === "board" && focusName && focusedWorking && focusedBand && (
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
      {place === "memory" && (
        <Memory
          native={NATIVE}
          now={now}
          tab={memoryTab}
          onTab={setMemoryTab}
          onOpenRun={(runId) => setWatching(runId)}
        />
      )}
      {/* WHAT IS NOT SAVED YET IS THE TREE'S, so it hangs under the tree and
          not under a terminal — three clicks in, which is where the two most
          urgent things this product knows had ended up. */}
      {place === "changes" && (
        <div className="section">
          <div className="section__body">
            {standingIn === null ? (
              <p className="world__mute">
                No tree open: what changed is a question about one.
              </p>
            ) : (
              <ChangesScreen
                key={standingIn.root}
                root={standingIn.root}
                name={standingIn.name}
              />
            )}
          </div>
        </div>
      )}
      {place === "ledger" && (
        <div className="section">
          <div className="section__body">
            <LedgerBrowser native={NATIVE} onTable={setLedgerTable} />
          </div>
        </div>
      )}
      {place === "sailor" && (
        <SailorScreen
          native={NATIVE}
          now={now}
          tab={sailorTab}
          onTerminalOpened={() => {
            setPlace("terminals");
            setTerminalsTab("live");
          }}
        />
      )}
      {/* THE TERMINALS STAY MOUNTED BEHIND THE OTHER PLACES, like the canvas:
          unmounting the screen would destroy every emulator, and a session
          would come back blank while the process inside is alive. */}
      <TerminalsSection
        native={NATIVE}
        now={now}
        shown={place === "terminals"}
        tab={terminalsTab}
        onTab={setTerminalsTab}
        ceiling={ceilingOf(flows)}
        onProjectChanged={() => readFlows(() => true)}
        onCount={setTerminalCount}
        onList={setOpenTerminals}
      />

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
      <div className="body" hidden={place !== "board"}>


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
                  if (node.type !== "step") return "var(--line)";
                  const data = node.data as StepNodeData;
                  const state = stepStates.get(nodeId(data.flowName, data.step.id))?.state ?? data.run?.state;
                  return STATE_COLOR[state ?? "idle"];
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
              places={places}
            />
          )}
        </main>

        {/* The right column stands when it has a step to describe: with flows
            and no step chosen, 288px stood for an invitation while the canvas
            beside it was cut mid-word. */}
        {selectedData !== undefined && (
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
            ) : null}
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
          onStop={() => stopRun(watched.run_id)}
        />
      )}
      </StepUsageContext.Provider>
      </WireContext.Provider>
      </StepRunContext.Provider>
      </RunContext.Provider>
      </div>
      </div>
    </div>
    </TooltipProvider>
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
          aria-label="name of the flow"
          onChange={(event) => setNameDraft(event.target.value)}
          onBlur={() => onRename(nameDraft)}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
      ) : (
        <span className="focusbar__name" title="the name is the file's: it is chosen before saving">
          {name}
        </span>
      )}
      <input
        className="focusbar__desc-input"
        value={descDraft}
        aria-label="description of the flow"
        placeholder="what this flow is for"
        onChange={(event) => setDescDraft(event.target.value)}
        onBlur={() => onDescription(descDraft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
      />
      <div className="focusbar__spacer" />
      {error && <span className="focusbar__error">{error}</span>}
      {/* DELETING IS NOT WHAT THIS BAR IS FOR. A red button beside the name of
          the thing it destroys is the loudest object on the screen, and it is
          the one gesture nobody comes here to make. It keeps its place — one
          click away, under the mark that always means «what else can I do». */}
      <DropdownMenu>
        <Tooltip>
          <TooltipTrigger asChild>
            <DropdownMenuTrigger asChild disabled={busy}>
              <button type="button" className="focusbar__more" aria-label="more for this flow">
                ⋯
              </button>
            </DropdownMenuTrigger>
          </TooltipTrigger>
          {/* The mark has no word beside it, so it needs one the instant the
              pointer arrives: ban 5 does not stop at colour. */}
          <TooltipContent side="bottom">More for this flow</TooltipContent>
        </Tooltip>
        <DropdownMenuContent align="end">
          <DropdownMenuItem variant="destructive" onSelect={onDelete}>
            {neverSaved ? "Discard flow" : "Delete flow"}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
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
 * How far a run has got, folded from its own facts. THE SNAPSHOT CARRIES NO
 * COUNTERS, and the ledger's undercount mid-run: so the denominator comes from
 * the flow on screen and the numerator from the events — the same fold the
 * canvas uses to colour its nodes.
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

interface TopBarProps {
  /**
   * Whether the board — and with it the column — is the place in view. The bar
   * is the program's and is drawn everywhere; the line about focusing a flow
   * belongs to one place only.
   */
  onBoard: boolean;
  /** Where the person is: the section, and the entry inside it. */
  crumbs: string[];
  /** What runs, what it costs, who as: drawn from every place. */
  chips?: ReactNode;
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
  onBoard,
  crumbs,
  chips,
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

      <nav className="topbar__crumbs" aria-label="where you are">
        {crumbs.map((crumb, index) => (
          <span className="topbar__crumb" key={`${index}-${crumb}`}>
            {crumb}
          </span>
        ))}
      </nav>

      {/* A LINE THAT NAMES THE COLUMN IS SILENT WHERE THERE IS NO COLUMN. The
          window opens away from the board, which is the only place holding
          one, and six places out of seven were being sent somewhere they
          are not. */}
      {flowName === null ? (
        onBoard && <span className="topbar__none">no flow in focus — pick one in the rail</span>
      ) : (
        <span className="topbar__flow">
          <span className="topbar__steps">{steps} steps</span>
          {dirty && (
            <span className="topbar__dirty">
              <span className="topbar__dot" />
              unsaved changes
            </span>
          )}
        </span>
      )}

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

      {chips}

      <button
        type="button"
        className="topbar__save"
        onClick={onSave}
        disabled={flowName === null || !dirty || busy}
      >
        {busy ? "Saving…" : "Save"}
      </button>
      {/* THE ACCENT MEANS «THE ACTION», and this is the action. Not a green:
          green is a step that went well, and prohibition 4 keeps the state
          colours for states. */}
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

