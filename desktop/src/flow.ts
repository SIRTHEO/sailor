// I tipi del flusso, ricalcati su `crates/flow`: la finestra deve parlare la
// stessa lingua del motore, o le due verità divergono senza che nessuno lo veda.
// Chi cambia `flow::Step` in Rust cambia anche questo file.

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
  /** I parametri dichiarati dal passo: vincono sulle chiavi ricevute in ingresso. */
  with?: Record<string, unknown> | null;
  when: Condition | null;
  /** Nome stabile dell'azione, risolto dal registro. Mai codice. */
  action: string;
  max_attempts: number;
}

export interface Graph {
  steps: Step[];
  skippable_dependencies?: DependencyEdge[];
}

/** Un file di flusso: il grafo più i valori con cui parte. */
export interface FlowFile {
  id: string;
  description: string;
  graph: Graph;
  inputs: Record<string, unknown>;
}

/**
 * Un flusso che non si carica non sparisce: arriva qui col suo motivo.
 * Il contrario è il difetto che rende un elenco corto senza dirlo.
 */
export interface BrokenFlow {
  name: string;
  reason: string;
}

export type FlowEntry =
  | { state: "loaded"; flow: FlowFile }
  | { state: "broken"; broken: BrokenFlow };

// ── come finisce un passo, e come si vede ────────────────────────────────

/**
 * I finali non sono intercambiabili: «fermo al tetto dei tentativi» non è
 * «rotto» — nessuno lo ritenterà — e «aspetta una persona» non è un guasto.
 */
export type StepState =
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
  /** Presente solo mentre un agente tiene il passo. */
  held_by_pid?: number;
  elapsed_secs?: number;
}

/** La specie dice cosa fa Sailor se il passo cade e l'effetto resta ignoto. */
export type StepSpecies = "repeatable" | "compensable" | "hand_to_human";

/** Le famiglie di nodo che la cassetta dei passi offre. */
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
 * Da quale azione nasce quale famiglia. Il motore ne registra due
 * (`external_engine`, `shell_check`); le altre sono la lista della spesa, e
 * finché non esistono un flusso che le nomina non parte.
 *
 * Una sola mappa, non uno switch: la cassetta dei passi e il pannello di
 * modifica leggono la stessa vocabolario invece di ricopiarlo.
 */
const ACTION_KIND: Record<string, StepKind> = {
  external_engine: "engine",
  shell_check: "check",
  pane_until_idle: "wait",
  signal_is_gone: "wait",
  deposit_write: "deposit",
  pane_send: "gesture",
  hand_to_human: "human",
  subflow: "subflow",
};

export function kindOf(action: string): StepKind {
  return ACTION_KIND[action] ?? "check";
}

/** I nomi di azione che il vocabolario conosce oggi, per il suggerimento nel pannello. */
export const KNOWN_ACTIONS: string[] = Object.keys(ACTION_KIND);

/**
 * L'azione con cui nasce un passo creato dalla cassetta, una per famiglia.
 * «trigger» e «branch» non compaiono: nessuna azione vi si risolve ancora nel
 * registro, e inventarne una vorrebbe dire scrivere un nome invece che
 * leggerlo da lì — la cassetta non offre quelle due famiglie finché non
 * esistono davvero.
 */
export const DEFAULT_ACTION_FOR_KIND: Partial<Record<StepKind, string>> = {
  engine: "external_engine",
  check: "shell_check",
  wait: "pane_until_idle",
  deposit: "deposit_write",
  gesture: "pane_send",
  human: "hand_to_human",
  subflow: "subflow",
};
