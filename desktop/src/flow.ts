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
  /**
   * Quanto una corsa di questo flusso può spendere, in micro-unità: un milione
   * è un'unità di valuta.
   *
   * `null` o assente vuol dire «nessun tetto», e NON zero — `0` è un flusso a
   * cui qualcuno ha detto di non spendere niente, e si ferma prima della prima
   * chiamata a pagamento. Il tetto si misura sui costi che i motori dichiarano:
   * chi non li dichiara lascia righe fuori dal conto, e la corsa fermata lo
   * scrive nel proprio motivo.
   */
  spend_cap_micros?: number | null;
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

// ── quanto è costata una corsa ──────────────────────────────────────────────
// Ricalcati su `crates/ui/src/dashboard.rs`, con la stessa disciplina dei tipi
// qui sopra: chi cambia l'uno cambia l'altro.

/**
 * I conteggi di una corsa.
 *
 * `null` NON ESISTE QUI, E NON È UNA SVISTA: questi sono totali, e un totale di
 * cose sconosciute è zero. Quello che non si sa lo dicono `callsWithoutTokens` e
 * `callsWithoutCost` — chi mostra una somma senza mostrare anche quei due numeri
 * sta presentando una cifra che nasconde ciò che le manca.
 */
export interface TokenTotals {
  input_tokens: number;
  output_tokens: number;
  /** Letti dalla cache: costano una frazione dell'ingresso. */
  cached_tokens: number;
  /** Scritti nella cache: costano PIÙ dell'ingresso, ed è la voce che sorprende. */
  cache_write_tokens: number;
  /** Il totale di chi non separa i due lati, tenuto a parte per non contare due volte. */
  total_tokens_only: number;
  cost_micros: number;
  calls: number;
  calls_without_tokens: number;
  calls_without_cost: number;
}

/** Una chiamata a un motore, come la registra il deposito. */
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
  /** Quanto il motore ha dichiarato di suo: se diverge dal nostro, si vede. */
  declared_cost_micros: number | null;
  error_type: string | null;
  started_at: number;
  ended_at: number | null;
}

/** Una corsa vista dal lato della spesa. */
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
 * Vero se questi totali nascondono qualcosa. È la stessa regola di
 * `TokenTotals::is_partial` in Rust, e la finestra deve dirlo a schermo: un
 * totale parziale che tace è peggio del non averlo.
 */
export function totalsArePartial(totals: TokenTotals): boolean {
  return totals.calls_without_tokens > 0 || totals.calls_without_cost > 0;
}
