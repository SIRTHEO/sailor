// The flow types, mirrored from `crates/flow`: the window must speak the
// engine's language, or the two truths drift with nobody seeing it. Whoever
// changes `flow::Step` in Rust changes this file too.

export type ValueSchema =
  | { type: "any" }
  | { type: "null" }
  | { type: "boolean" }
  | { type: "number" }
  | { type: "string" }
  | { type: "one_of"; values: unknown[] }
  | { type: "array"; items: ValueSchema }
  | {
      type: "object";
      properties: Record<string, ValueSchema>;
      required: string[];
      allow_extra: boolean;
    };

export type Condition =
  | { kind: "equals"; value: unknown }
  | { kind: "pointer_equals"; pointer: string; value: unknown }
  | { kind: "pointer_exists"; pointer: string };

export interface DependencyEdge {
  step: string;
  dependency: string;
}

export interface Step {
  id: string;
  deps: string[];
  input_schema: ValueSchema;
  output_schema: ValueSchema;
  /** The step's declared params: they beat the keys received as input. */
  with?: Record<string, unknown> | null;
  when: Condition | null;
  /** The action's stable name, resolved by the registry. Never code. */
  action: string;
  max_attempts: number;
}

export interface Graph {
  steps: Step[];
  skippable_dependencies?: DependencyEdge[];
}

/**
 * How many steps a flow has, phrased for a reader. It lives here and not in the
 * two places that show it, because a plural written twice is a plural that goes
 * wrong in one place only, and nobody notices from the other.
 */
export function stepCountLabel(count: number): string {
  return `${count} ${count === 1 ? "step" : "steps"}`;
}

/** A flow file: the graph plus the values it starts with. */
export interface FlowFile {
  id: string;
  description: string;
  graph: Graph;
  inputs: Record<string, unknown>;
  /**
   * What a run may spend, in micro-units. `null` or absent means "no cap", NOT
   * zero — `0` is a flow told to spend nothing, and it stops before the first
   * paid call. The cap is measured on the costs the engines declare; whoever
   * does not declare them leaves rows out of the tally.
   */
  spend_cap_micros?: number | null;
}

/**
 * A flow that will not load does not vanish: it arrives here with its reason.
 * The opposite is the defect that shortens a list without saying so.
 */
export interface BrokenFlow {
  name: string;
  reason: string;
}

/**
 * Which of the sources a flow came from: shipped inside the binary, yours in
 * the home, or the project's. On a name clash the most specific wins, and the
 * origin is the only thing that says which of two same-named flows is running.
 */
export type Origin = string;

export type FlowEntry =
  | { state: "loaded"; flow: FlowFile; origin: Origin }
  | { state: "broken"; broken: BrokenFlow; origin: Origin };

// ── how a step ends, and how it is shown ─────────────────────────────────

/**
 * The endings are not interchangeable: stopped at the retry cap is not broken
 * — nobody will retry it — and waiting on a person is not a failure.
 */
export type StepState =
  /** No run has touched this step: not waiting on anything, simply never run. */
  | "idle"
  | "waiting"
  | "running"
  | "went"
  | "broke"
  | "capped"
  | "handed_to_human";

export interface StepRun {
  step_id: string;
  state: StepState;
  attempt: number;
  /** Present only while an agent holds the step. */
  held_by_pid?: number;
  elapsed_secs?: number;
}

/** The kind says what Sailor does if the step falls and its effect is unknown. */
export type StepSpecies = "repeatable" | "compensable" | "hand_to_human";

/** The node families the step toolbox offers. */
export type StepKind =
  | "trigger"
  | "engine"
  | "check"
  | "wait"
  | "branch"
  | "deposit"
  | "gesture"
  | "human"
  | "subflow";

/**
 * Which action gives which family, in one map rather than a switch, so the
 * toolbox and the editor panel share a vocabulary instead of copying it.
 * **EVERY NAME HERE MUST BE AN ACTION THE ENGINE REALLY REGISTERS**: an
 * invented one puts a button in the toolbox that makes a node which will not
 * save. The keeper is outside both copies —
 * `the_window_vocabulary_names_only_actions_the_engine_registers` in
 * `desktop/src-tauri/src/flows.rs` reads this file against the registry, both
 * ways, because two hand-written maps can be wrong together.
 */
const ACTION_KIND: Record<string, StepKind> = {
  // Where the signal comes from. Without this entry the real `trigger` steps
  // fall back to the check family and draw as control nodes.
  trigger: "trigger",
  external_engine: "engine",
  shell_check: "check",
  detect_tools: "check",
  tool_needs: "check",
  mcp_ready: "check",
  mcp_ask: "gesture",
  // The step that starts nothing: it describes the work and leaves it to
  // whoever is already alive in the terminal.
  handed_to_agent: "human",
  history_ask: "deposit",
  fault_list: "deposit",
  fault_record: "deposit",
  store_read: "deposit",
  store_write: "deposit",
  store_list: "deposit",
  work_claim: "deposit",
  work_release: "deposit",
  work_survey: "deposit",
  // The relay's four nodes. Measuring produces a verdict a `when` reads, so it
  // draws as a check; typing and emptying reach a live session, which is a
  // gesture; taking the mandate waits for whoever is alive in there to write
  // it, which is the same family as handing work to them.
  measure_terminal: "check",
  type_into_terminal: "gesture",
  empty_terminal: "gesture",
  take_mandate: "human",
  // The only step that writes a proposal onto the tree. It draws as a gesture
  // on the world, which is what it is.
  apply_patch: "gesture",
  subflow: "subflow",
};

export function kindOf(action: string): StepKind {
  return ACTION_KIND[action] ?? "check";
}

/** The action names the vocabulary knows today, for the panel's suggestions. */
export const KNOWN_ACTIONS: string[] = Object.keys(ACTION_KIND);

/**
 * The action a toolbox-created step is born with, one per family. Every value
 * must be an action the engine registers, and a test checks it. The wait and
 * branch families are absent because no action resolves to them, and inventing
 * one would mean writing a name instead of reading it from the registry.
 */
export const DEFAULT_ACTION_FOR_KIND: Partial<Record<StepKind, string>> = {
  trigger: "trigger",
  engine: "external_engine",
  check: "shell_check",
  gesture: "mcp_ask",
  human: "handed_to_agent",
  deposit: "store_write",
  subflow: "subflow",
};

// ── what a run cost ─────────────────────────────────────────────────────────
// Mirrored from `crates/ui/src/dashboard.rs`, under the same discipline as the
// types above: whoever changes one changes the other.

/**
 * A run's counts. `null` DOES NOT EXIST HERE, AND THAT IS NOT AN OVERSIGHT:
 * these are totals, and a total of unknown things is zero. What is not known is
 * said by `callsWithoutTokens` and `callsWithoutCost` — a sum shown without
 * those two hides what it is missing.
 */
export interface TokenTotals {
  input_tokens: number;
  output_tokens: number;
  /** Read from cache: a fraction of the input price. */
  cached_tokens: number;
  /** Written to cache: they cost MORE than input, which is the surprising one. */
  cache_write_tokens: number;
  /** The total from engines that do not split the two sides, kept apart so it
   * is not counted twice. */
  total_tokens_only: number;
  cost_micros: number;
  calls: number;
  calls_without_tokens: number;
  calls_without_cost: number;
}

/** A call to an engine, as the store records it. */
export interface CallView {
  call_id: string;
  step_id: string | null;
  cli: string;
  actual_model: string;
  input_tokens: number | null;
  output_tokens: number | null;
  cached_tokens: number | null;
  cache_write_tokens: number | null;
  cache_write_long_tokens: number | null;
  total_tokens: number | null;
  cost_micros: number | null;
  /** What the engine declared itself: if it diverges from ours, it shows. */
  declared_cost_micros: number | null;
  error_type: string | null;
  started_at: number;
  ended_at: number | null;
}

/** A run seen from the spending side. */
export interface RunUsage {
  run_id: string;
  entity: string;
  status: string;
  total_cost_micros: number;
  steps_total: number;
  steps_went: number;
  steps_broke: number;
  tokens: TokenTotals;
  tokens_by_model: Record<string, TokenTotals>;
  calls: CallView[];
}

/**
 * True if these totals hide something. Same rule as `TokenTotals::is_partial`
 * in Rust, and the window has to say it on screen: a partial total that keeps
 * quiet is worse than no total.
 */
export function totalsArePartial(totals: TokenTotals): boolean {
  return totals.calls_without_tokens > 0 || totals.calls_without_cost > 0;
}
