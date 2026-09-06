// The bridge between the canvas and the engine: from here, and only from here,
// the window asks for real data.
//
// **WHY NOT `@tauri-apps/api`.** The normal way, to be taken back as soon as it
// can be: `npm install` did not pass because the npm cache (`~/.npm`) belongs
// to another user and wants a `sudo chown` a session cannot give. So the shell
// exposes `window.__TAURI__` (`withGlobalTauri` in `tauri.conf.json`): the same
// call without the package and without the types, declared by hand below.
//
// **OUTSIDE THE WINDOW THERE IS NO ENGINE**, and it is not a fault: `npm run
// dev` alone serves the canvas in a browser, where `window.__TAURI__` does not
// exist. There the example data shows, and the reader must tell so from the
// window rather than from the code.

import type { FlowEntry, FlowFile, Origin, RunUsage } from "./flow";
import { parseTools, publishTools, type Tool } from "./tools";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface TauriGlobal {
  core?: { invoke?: Invoke };
}

/** How the shell is called. One copy, so nobody grows a second contract. */
export function invoker(): Invoke | null {
  const tauri = (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__;
  return tauri?.core?.invoke ?? null;
}

/** True when the canvas runs inside the native shell, false in a browser. */
export function insideTheWindow(): boolean {
  return invoker() !== null;
}

/**
 * The declared flows, read from disk by the engine.
 *
 * An error is not swallowed: the caller decides whether to show the example or
 * the fault, but must know which of the two it is looking at.
 */
export async function loadFlows(): Promise<FlowEntry[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to ask");
  return invoke<FlowEntry[]>("flows");
}

/**
 * Writes a flow to disk, through the engine. `save_flow` is born alongside this
 * panel, in another worksite on the same shell: until that side answers, this
 * call fails with a readable error instead of staying mute.
 */
export async function saveFlow(flow: FlowFile): Promise<Origin> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to save to");
  // The origin comes back because the engine is the only one that knows it: a
  // flow that has never been saved belongs nowhere, and after this call it
  // belongs somewhere precise. Guessing here would put the wrong heading over
  // it in the column, which is the one place the answer is read.
  return invoke<Origin>("save_flow", { flow });
}

/**
 * The tools the engine finds on this machine: the command lines with an AI
 * behind them, the MCP servers, the binaries a step can invoke.
 *
 * **WHICH TOOL EXISTS IS NOT THE WINDOW'S TO KNOW**, and must not be: the
 * engine discovers it and declares it here. `discover_tools` is born in another
 * worksite while this panel is written — until that command answers, this call
 * fails, and the panel says so instead of staying blank.
 *
 * The answer is read with `parseTools`, which discards an entry without an
 * identifier rather than trusting the shape it received.
 */
export async function discoverTools(): Promise<Tool[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to ask for the tools");
  const tools = parseTools(await invoke<unknown>("discover_tools"));
  // The outcome lands in the shared registry so a node on the canvas can show
  // its own tool's mark and state without the discovery being handed to it from
  // hand to hand: it is machine data, not step data, and letting it descend the
  // chain would mean rewriting whoever builds that chain. There stays one single
  // discovery — this one.
  publishTools(tools);
  return tools;
}

/** Deletes a flow from disk, through the engine. Same premise as `saveFlow`. */
export async function deleteFlow(name: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to delete from");
  await invoke<void>("delete_flow", { name });
}

// ── starting a flow, and watching it run ─────────────────────────────────

/**
 * One fact of a run, numbered.
 *
 * **THE NUMBER IS THERE SO NOTHING IS TOLD TWICE.** Whoever opens the view asks
 * first for what already happened and then listens: between the two calls the
 * run goes on, and a fact can arrive by both roads. `seq` grows by one per run,
 * and a listener drops what it has already seen instead of trusting the order
 * of arrival.
 */
export interface RunEvent {
  run_id: string;
  seq: number;
  kind: "step_started" | "step_text" | "step_closed" | "run_ended" | "stop_requested" | "note";
  at: number;
  step_id: string | null;
  payload: unknown;
}

export interface RunSnapshot {
  run_id: string;
  flow: string;
  started_at: number;
  status: string;
  events: RunEvent[];
}

export interface StartedRun {
  run_id: string;
  flow: string;
  started_at: number;
}

/**
 * Where the text of whoever presses the button ends up — or why there is no
 * room for it. The shell decides, not the window: a second rule written here
 * would drift from the first without anyone noticing.
 */
export type MandateTarget =
  | { kind: "field"; step: string; field: string }
  | { kind: "none"; why: string };

export interface FlowTrigger {
  flow: string;
  roots: string[];
  mandate: MandateTarget;
  scheduled: boolean;
}

/** How a flow is triggered: where it starts, and whether it takes a mandate. */
export async function flowTrigger(name: string): Promise<FlowTrigger> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to trigger");
  return invoke<FlowTrigger>("flow_trigger", { name });
}

/**
 * Starts a flow. It returns as soon as the run is started, not when it ends: a
 * flow that calls an agent can last half an hour, and the button must not stay
 * pressed all that time.
 */
export async function startRun(name: string, mandate: string | null): Promise<StartedRun> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to run it");
  return invoke<StartedRun>("start_run", { name, mandate });
}

/**
 * Asks a run to stop before its next step. The step running now finishes:
 * the engine cannot take a step back from an agent already at work.
 */
export async function stopRun(runId: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no run to stop");
  await invoke<void>("stop_run", { runId });
}

/** One of the places the engine reads flows from, and what it found there. */
export interface FlowPlace {
  origin: string;
  path: string;
  exists: boolean;
  count: number;
}

/**
 * Where the engine looked for flows. Worth asking when the board is empty:
 * an empty list without the places it came from cannot be told from a fault.
 */
export async function flowPlaces(): Promise<FlowPlace[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no disk to look at");
  return invoke<FlowPlace[]>("flow_places");
}

/** What the supervisor of the live window says about the build it is showing. */
export interface LiveStatus {
  state: "building" | "running" | "build_failed" | "ready";
  /** Empty when all is well; the compiler's output when it is not. */
  message: string;
  changed_at: number;
  running_since: number | null;
}

/** `null` when no supervisor has written a status: the window is not in live mode. */
export async function liveStatus(): Promise<LiveStatus | null> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no supervisor");
  return invoke<LiveStatus | null>("live_status");
}

/** Asks for the build that is waiting. **This window ends when it is granted.** */
export async function takeNewBuild(): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no supervisor");
  return invoke<void>("take_new_build");
}

/** What one flow's schedule came to at one beat. */
export interface BeatDecision {
  flow: string;
  verdict: "ran" | "held" | "broke";
  run_id?: string;
  /** Why it was held, or why it broke. Empty on `ran`. */
  why?: string;
}

/** The last beat, whole: **a beat says what it did not do, and why.** */
export interface BeatReport {
  at: number;
  decisions: BeatDecision[];
}

/** `null` before the first beat of this window, which comes within a second. */
export async function beatReport(): Promise<BeatReport | null> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no beat");
  return invoke<BeatReport | null>("beat_report");
}

/** A step of a run that waits for a person, with what it asks. */
export interface HandedStep {
  step_id: string;
  /** Whom it was offered to: a label for the reader, not a credential. */
  holder: string;
  mandate: string;
  since: number;
  /** The tree the run was born in; `null` when it was born outside every one. */
  worktree: string | null;
}

export async function handedSteps(runId: string): Promise<HandedStep[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to read");
  return invoke<HandedStep[]>("handed_steps", { runId });
}

/** Takes a handed step as the person at this machine; answers the engine's report. */
export async function takeHandedStep(runId: string, stepId: string): Promise<string> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no step to take");
  return invoke<string>("take_handed_step", { runId, stepId });
}

/** Closes a handed step with the outcome declared, and the run resumes. */
export async function closeHandedStep(
  runId: string,
  stepId: string,
  outcome: "went" | "broke",
  said: string,
): Promise<string> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no step to close");
  return invoke<string>("close_handed_step", { runId, stepId, outcome, said });
}

/** Everything a run has said so far, for whoever looks in now. */
export async function runSnapshot(runId: string): Promise<RunSnapshot> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no run to watch");
  return invoke<RunSnapshot>("run_snapshot", { runId });
}

/**
 * The runs the shell knows.
 *
 * **IT IS FOR WHOEVER RELOADS THE PAGE** while a flow runs: the run lives in
 * the shell and goes on, but the canvas would start again unaware of it.
 * Without this list, work in progress would turn invisible while still alive.
 */
export async function knownRuns(): Promise<RunSnapshot[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no runs to list");
  return invoke<RunSnapshot[]>("known_runs");
}

/**
 * An open run, whoever started it. Mirrors `OpenRun` in
 * `desktop/src-tauri/src/run.rs`: whoever changes one changes the other.
 */
export interface OpenRun {
  run_id: string;
  entity: string;
  /** «working» is at work; «waiting» is stopped and resumes only if you act. */
  state: "working" | "waiting";
  open_steps: number;
  /** **Which** steps are open, and for how long. Empty for a waiting run. */
  open_now: Array<{ step_id: string; attempt: number; open_for_secs: number }>;
  /** Since when this state has lasted, in seconds from the epoch. */
  since: number;
  /** True if this window is the one that started it. */
  started_here: boolean;
  /** Steps with an outcome already, counted once each. */
  steps_done: number;
  /** Steps the flow declares; null when the flow cannot be read back. */
  steps_total: number | null;
}

/**
 * Every open run on the machine, not only this window's.
 *
 * **THIS IS NOT `knownRuns` WITH MORE ROWS.** That one reads the shell's memory
 * and knows only what this window started; this one asks the ledger, and also
 * sees a run started from the terminal, from another window, or from a nightly
 * schedule. It is the difference between a screen that says «what is happening»
 * and one that says «what I did» while believing they are the same thing.
 */
export async function openRuns(): Promise<OpenRun[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to ask");
  return invoke<OpenRun[]>("open_runs");
}

// ── what the board could say, and the window now says ────────────────────

/** A day's summary. Mirrors `DaySummary` in `board.rs`. */
export interface DaySummary {
  ledger_present: boolean;
  runs: number;
  went: number;
  broke: number;
  still_open: number;
  input_tokens: number;
  output_tokens: number;
  cached_tokens: number;
  cache_write_tokens: number;
  cost_micros: number;
  /** Model calls that reported no tokens. */
  unmeasured: number;
  /** Model calls that reported no price. */
  unpriced: number;
  tokens_by_model: Record<string, number>;
}

/**
 * The summary of the runs begun after a given instant.
 *
 * **THIS FUNCTION COMPUTES THE INSTANT**, because «today» is a local calendar
 * day and the timezone is known to the system that draws, not to the engine.
 * The sum is the engine's instead, made once: two sums in two languages would
 * give two figures and nobody would know which to believe.
 */
export async function todaySummary(): Promise<DaySummary> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to sum up");
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  return invoke<DaySummary>("day_summary", { since: Math.floor(midnight.getTime() / 1000) });
}

/** A run in the history. Mirrors `ExecutionView` in `crates/ui/src/dashboard.rs`. */
export interface Execution {
  run_id: string;
  kind: string;
  entity: string;
  /** The tree it was born in. `null`: outside every workspace, or before v10. */
  worktree: string | null;
  status: string;
  started_at: number;
  ended_at: number | null;
  duration_secs: number | null;
  total_cost_micros: number;
  error: string | null;
  steps_total: number;
  steps_went: number;
  steps_broke: number;
  steps_retried: number;
  steps_open: Array<{ step_id: string; attempt: number; started_at: number; open_for_secs: number }>;
  tokens: {
    input_tokens: number;
    output_tokens: number;
    cached_tokens: number;
    cache_write_tokens: number;
    cost_micros: number;
    calls: number;
    calls_without_tokens: number;
    calls_without_cost: number;
  };
  /** Tokens seen per model, already summed by the engine. */
  tokens_by_model: Record<string, { input_tokens: number; output_tokens: number; cached_tokens: number; cache_write_tokens: number; cost_micros: number; calls: number }>;
  calls: ModelCall[];
}

/**
 * A model call inside a run.
 *
 * **`declared_cost_micros` IS NOT A DUPLICATE OF `cost_micros`.** One is the
 * price Sailor computes from the tokens, the other is what the engine says it
 * spent. Keeping them side by side is the only way to notice that one of the
 * two is wrong — exactly the defect the survey found invisible to Langfuse,
 * LangSmith and Phoenix, all three showing wrong numbers with authority.
 */
export interface ModelCall {
  call_id: string;
  step_id: string | null;
  purpose: string;
  cli: string;
  requested_model: string;
  actual_model: string;
  input_tokens: number | null;
  output_tokens: number | null;
  cached_tokens: number | null;
  cache_write_tokens: number | null;
  total_tokens: number | null;
  turns: number | null;
  cost_micros: number | null;
  /** What the engine says it spent, when it says so. */
  declared_cost_micros: number | null;
  error_type: string | null;
  started_at: number;
  ended_at: number | null;
}

/** Every run the ledger remembers, newest first. */
export async function executionHistory(): Promise<Execution[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no history to read");
  return invoke<Execution[]>("execution_history");
}

/** A thing installed on this machine. Mirrors `Entry` in `crates/inventory`. */
export interface InstalledEntry {
  kind: "skill" | "agent" | "command" | "rule" | "hook";
  name: string;
  description: string;
  /** Where it comes from: `home`, `plugin <name>`, `repo <name>`. */
  origin: string;
  path: string;
  reach: { state: "active" } | { state: "inactive"; reason: string } | { state: "unknown"; reason: string };
  /** The model can invoke it by itself, or only the person who types. */
  by_model: boolean;
}

export interface Installed {
  entries: InstalledEntry[];
  /** Where it really looked: a list that does not say cannot be contradicted. */
  roots: string[];
  stale_plugin_copies: number;
}

/** Skills, agents, commands, rules, hooks: what is on this machine. */
export async function machineInventory(): Promise<Installed> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: the engine takes the census");
  return invoke<Installed>("machine_inventory");
}

/** A command of the command line, as the binary declares it. */
export interface CommandDoc {
  /** The name that is typed: `flow`, `step`, `release`. */
  name: string;
  /** The line that says what it is for — the same as `sailor --help`. */
  description: string;
  /** The forms, each already split into what is typed and what it says. */
  usage: FormDoc[];
}

/**
 * One usage form, in its two halves. **`form` is not translated and `says`
 * is**: translating the first would break the command it describes.
 */
export interface FormDoc {
  /** What is typed: `sailor flow run <name> [mandate]`. */
  form: string;
  /** What it does. Empty when the shape says it all on its own. */
  says: string;
}

/**
 * The commands this Sailor can run.
 *
 * **THERE IS NO LIST OF COMMANDS IN TYPESCRIPT, AND THAT IS THE POINT.**
 * Writing them here would have been half an hour of work and a page that drifts
 * from the binary at the first option added: fault 10, which in this repo has
 * already come back five times — the last the same day, on the vocabulary of
 * the actions. `crates/sailor` is lib+bin on purpose, and `manual` translates
 * the shape alone.
 */
export async function manual(): Promise<CommandDoc[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: the binary declares the commands");
  return invoke<CommandDoc[]>("manual");
}

type Unlisten = () => void;

interface EventGlobal {
  listen?: <T>(event: string, handler: (event: { payload: T }) => void) => Promise<Unlisten>;
}

/**
 * Listens to what happens in the runs.
 *
 * **A LISTENER THAT DOES NOT ATTACH MUST SAY SO.** Returning `null` in silence
 * is what this function used to do, and it cost a view showing «running for
 * 00:30» on a run long since finished: the channel was not there, nobody knew,
 * and the window kept drawing the last state it had received. The caller must
 * be able to tell the reader that what they see does not refresh by itself.
 */
export async function listenToRuns(
  handler: (event: RunEvent) => void,
): Promise<{ stop: Unlisten } | { why: string }> {
  const tauri = (window as unknown as { __TAURI__?: TauriGlobal & { event?: EventGlobal } }).__TAURI__;
  if (!tauri) return { why: "outside the native shell: no channel of events" };
  const listen = tauri.event?.listen;
  if (!listen) {
    return {
      why: "the shell exposes no «event.listen»: the view refreshes by asking instead of listening",
    };
  }
  try {
    const stop = await listen<RunEvent>("sailor://run", (event) => handler(event.payload));
    return { stop };
  } catch (error) {
    return { why: String(error) };
  }
}

/** One fact of the shell, on the one channel every fact crosses. */
export interface SailorEvent {
  kind: string;
  at: number;
  payload: unknown;
}

/**
 * Subscribes to the one channel. Outside the shell, or without «event.listen»,
 * the reason comes back and the caller asks once instead of listening.
 */
export async function listenToSailorEvents(
  handler: (event: SailorEvent) => void,
): Promise<{ stop: Unlisten } | { why: string }> {
  const tauri = (window as unknown as { __TAURI__?: TauriGlobal & { event?: EventGlobal } }).__TAURI__;
  if (!tauri) return { why: "outside the native shell: no channel of events" };
  const listen = tauri.event?.listen;
  if (!listen) return { why: "the shell exposes no «event.listen»" };
  try {
    const stop = await listen<SailorEvent>("sailor_event", (event) => handler(event.payload));
    return { stop };
  } catch (error) {
    return { why: String(error) };
  }
}

// ── what entered a node, over time ───────────────────────────────────────

/**
 * Which declared check refused a value, where in it, by which rule, and an
 * excerpt of what it saw. Structure beside the failure class, as the ledger
 * keeps it: the window shows the rule and the path without parsing prose.
 */
export interface Refusal {
  check: string;
  /** The path of the field that failed; empty when the whole value did. */
  path: string;
  rule: string;
  seen: string;
}

/**
 * The process a step started, as it was started: the program after resolution,
 * not the template, and its arguments. Each word is cut by the ledger, so a
 * prompt handed on the line does not become the row.
 */
export interface Ran {
  program: string;
  args: string[];
}

/**
 * One time a step was crossed.
 *
 * It comes from the ledger, not from this window's memory: the runs this window
 * started are a handful, the ones the node has seen pass can be hundreds —
 * started from the command line, from a schedule, or from a window closed
 * months ago.
 */
export interface StepPassage {
  run_id: string;
  attempt: number;
  started_at: number;
  ended_at: number | null;
  outcome: string | null;
  failure_class: string | null;
  refusal: Refusal | null;
  /** The program and the arguments the step started, after resolution. */
  ran: Ran | null;
  /** Where the run started from: the provenance, written by the system. */
  started_by: string;
  /** What entered this node, that time. */
  input: unknown;
  /** The mandate the run started with, if it carried one. */
  mandate: string | null;
  /** Who sent the signal, as the source knew it. */
  signal_who: string | null;
  /** Where it came from: the window, a panel, a session. */
  signal_where: string | null;
  said: string | null;
  output: unknown;
}

/** Everything that passed through a node, newest first. */
export async function stepHistory(flow: string, step: string, limit?: number): Promise<StepPassage[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to ask");
  return invoke<StepPassage[]>("step_history", { flow, step, limit: limit ?? null });
}

/**
 * What a run consumed: tokens, cache and money.
 *
 * **THE SUMS ARE NOT REDONE HERE.** The engine makes them (`ui::dashboard`),
 * the same code that serves the `sailor ui` page: two sums written in two
 * languages would give two figures, and nobody would know which to believe.
 *
 * `null` is not an error: it is a run the ledger has not projected yet, or a
 * ledger that does not exist because nothing was ever run.
 */
export async function runUsage(runId: string): Promise<RunUsage | null> {
  const invoke = invoker();
  if (!invoke) return null;
  return invoke<RunUsage | null>("run_usage", { runId });
}
