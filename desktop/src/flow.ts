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
 * Da quale azione nasce quale famiglia.
 *
 * **QUESTI SEDICI NOMI ESISTONO, E DEI PRECEDENTI CINQUE NO.** Fino al
 * 01/09/2026 questa mappa ne portava otto, e cinque erano inventati —
 * `pane_until_idle`, `signal_is_gone`, `deposit_write`, `pane_send`,
 * `hand_to_human`: zero occorrenze in tutto il Rust. Quattro stavano nella
 * cassetta dei passi, quindi premere «attesa», «deposito», «gesto» o «a una
 * persona» creava un nodo che poi **non si salvava**, con «il flusso usa
 * azioni che il motore non conosce». E dall'altro lato mancavano tredici
 * azioni su sedici: `kindOf` ripiega su «verifica», quindi i passi `trigger`
 * dei flussi veri si disegnavano come nodi di controllo, in silenzio. Il
 * commento che stava qui diceva «il motore ne registra due»: era vero fino al
 * 28/08 e nessuno l'ha più riletto.
 *
 * A tenerli allineati adesso è una prova che sta **fuori da tutte e due le
 * copie**: `crates/sailor/tests/one_action_list_for_the_window_and_the_engine.rs`
 * legge questo file e lo confronta col registro del motore **eseguito**, nei
 * due versi. Confrontare due mappe scritte a mano le lascerebbe sbagliare
 * insieme. Sta in `crates/sailor` e non in `desktop/src-tauri` perché quello
 * sta fuori dal workspace Rust: una prova scritta là non diventa mai rossa per
 * il gate.
 *
 * Una sola mappa, non uno switch: la cassetta dei passi e il pannello di
 * modifica leggono lo stesso vocabolario invece di ricopiarlo.
 *
 * **IL PERIMETRO DELLA GUARDIA, DETTO PERCHÉ NON SI CREDA PIÙ AMPIO DI COM'È.**
 * Il confronto è col registro **del motore** (`registry::default_registry`), non
 * con `flows.rs::action_registry()` — che è una **terza** copia, si costruisce
 * il registro a mano, e a quello chiede `save_flow` se un flusso si può
 * salvare. Le due liste non coincidono: `actions::register_default` ne porta
 * quattro, quindi in quest'albero un nodo `handed_to_agent`, `detect_tools`,
 * `subflow`, `history_ask` o `work_claim` si disegna con la famiglia giusta e
 * poi **viene respinto al salvataggio**. È il difetto di prima con un nome
 * nuovo, e questa guardia non lo vede: trovato da un giudice che non aveva
 * scritto questo lavoro, e scritto qui perché un perimetro non dichiarato è il
 * modo in cui un controllo verde si crede una promessa. Sparisce quando
 * `action_registry()` smette di costruirsi il registro da sé; finché non
 * succede, **il guasto 10 resta aperto** anche con questa prova verde.
 */
const ACTION_KIND: Record<string, StepKind> = {
  // Da dove arriva il segnale. Prima ricadeva su «verifica».
  trigger: "trigger",
  external_engine: "engine",
  // Le quattro che guardano il mondo senza toccarlo: una verifica di shell, il
  // censimento degli strumenti, «questi flussi girano qui?», e «questo server
  // MCP risponde?».
  shell_check: "check",
  detect_tools: "check",
  tool_needs: "check",
  mcp_ready: "check",
  mcp_ask: "gesture",
  // Il passo che non avvia niente: descrive il lavoro e lo lascia a chi è già
  // vivo nel terminale. È il nome vero di quello che qui si chiamava
  // `hand_to_human`.
  handed_to_agent: "human",
  // Tutto ciò che passa dal deposito: leggerne lo storico, scriverci, e dire
  // chi sta lavorando su cosa.
  history_ask: "deposit",
  store_read: "deposit",
  store_write: "deposit",
  store_list: "deposit",
  work_claim: "deposit",
  work_release: "deposit",
  work_survey: "deposit",
  subflow: "subflow",
};

export function kindOf(action: string): StepKind {
  return ACTION_KIND[action] ?? "check";
}

/** I nomi di azione che il vocabolario conosce oggi, per il suggerimento nel pannello. */
export const KNOWN_ACTIONS: string[] = Object.keys(ACTION_KIND);

/**
 * L'azione con cui nasce un passo creato dalla cassetta, una per famiglia.
 *
 * «attesa» e «ramo» non compaiono: **nessuna azione del motore si risolve in
 * quelle due famiglie**, e inventarne una vorrebbe dire scrivere un nome
 * invece che leggerlo da lì. La cassetta non le offre finché non esistono
 * davvero — e finché non esistono, un tasto che le creasse farebbe un nodo che
 * non si salva. «innesco» invece adesso c'è, ed è il primo passo di ogni
 * flusso vero di questa casa.
 *
 * Ogni valore qui è un nome di azione, quindi passa dallo stesso controllo di
 * `ACTION_KIND`: `every_action_the_palette_creates_is_registered`.
 */
export const DEFAULT_ACTION_FOR_KIND: Partial<Record<StepKind, string>> = {
  trigger: "trigger",
  engine: "external_engine",
  check: "shell_check",
  deposit: "store_write",
  gesture: "mcp_ask",
  human: "handed_to_agent",
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
