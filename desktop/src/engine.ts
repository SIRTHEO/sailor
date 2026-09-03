// Il ponte fra la tela e il motore: da qui, e solo da qui, la finestra chiede
// dati veri.
//
// PERCHÉ NON `@tauri-apps/api`. Sarebbe la via normale, ed è quella da
// riprendere appena si può: il 28/08/2026 `npm install` non è passato perché la
// cache di npm (`~/.npm`) appartiene a un altro utente e vuole un `sudo chown`
// che una sessione non può dare. Il guscio espone allora `window.__TAURI__`
// (`withGlobalTauri` in `tauri.conf.json`), che è la stessa chiamata senza il
// pacchetto e senza i tipi — dichiarati qui sotto a mano.
//
// FUORI DALLA FINESTRA NON C'È MOTORE, e non è un guasto: `npm run dev` da solo
// serve la tela in un browser, dove `window.__TAURI__` non esiste. Lì si vedono
// i dati di esempio, e chi guarda deve poterlo capire dalla finestra invece che
// dal codice.

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

/** Vero quando la tela gira dentro il guscio nativo, falso in un browser. */
export function insideTheWindow(): boolean {
  return invoker() !== null;
}

/**
 * I flussi dichiarati, letti dal disco dal motore.
 *
 * Un errore non si inghiotte: chi chiama decide se mostrare l'esempio o il
 * guasto, ma deve sapere quale dei due sta guardando.
 */
export async function loadFlows(): Promise<FlowEntry[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to ask");
  return invoke<FlowEntry[]>("flows");
}

/**
 * Scrive un flusso sul disco, tramite il motore. `save_flow` nasce insieme a
 * questo pannello, in un altro cantiere sullo stesso guscio: finché quel lato
 * non risponde, questa chiamata fallisce con un errore leggibile invece di
 * restare muta.
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
 * Gli strumenti che il motore trova su questa macchina: le righe di comando
 * con un'IA dietro, i server MCP, i binari che un passo può invocare.
 *
 * QUALE STRUMENTO ESISTA NON LO SA LA FINESTRA, e non deve saperlo: lo scopre
 * il motore e lo dichiara qui. `discover_tools` nasce in un altro cantiere
 * mentre questo pannello si scrive — finché quel comando non risponde questa
 * chiamata fallisce, e il pannello lo dice invece di restare bianco.
 *
 * La risposta si legge con `parseTools`, che scarta una voce senza
 * identificativo invece di fidarsi della forma ricevuta.
 */
export async function discoverTools(): Promise<Tool[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to ask for the tools");
  const tools = parseTools(await invoke<unknown>("discover_tools"));
  // L'esito si deposita nel registro condiviso perché un nodo sulla tela possa
  // mostrare il segno e lo stato del proprio strumento senza che la scoperta
  // gli venga passata di mano in mano: sono dati della macchina, non del passo,
  // e farli scendere lungo la catena vorrebbe dire riscrivere chi la costruisce.
  // La scoperta resta una sola — questa.
  publishTools(tools);
  return tools;
}

/** Cancella un flusso dal disco, tramite il motore. Stessa premessa di `saveFlow`. */
export async function deleteFlow(name: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to delete from");
  await invoke<void>("delete_flow", { name });
}

// ── far partire un flusso, e guardarlo correre ───────────────────────────

/**
 * Un fatto di una corsa, numerato.
 *
 * IL NUMERO SERVE A NON RACCONTARE DUE VOLTE LA STESSA COSA. Chi apre la vista
 * chiede prima quello che è già successo e poi si mette in ascolto: fra le due
 * chiamate la corsa continua, e un fatto può arrivare per tutte e due le
 * strade. `seq` cresce di uno per corsa, e chi ascolta scarta quello che ha già
 * visto invece di fidarsi dell'ordine di arrivo.
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
 * Dove finisce il testo di chi preme il pulsante — o perché non c'è posto.
 * Lo decide il guscio, non la finestra: una seconda regola scritta qui
 * divergerebbe dalla prima senza che nessuno se ne accorga.
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

/** Come si innesca un flusso: da dove parte, e se accetta una consegna. */
export async function flowTrigger(name: string): Promise<FlowTrigger> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no engine to trigger");
  return invoke<FlowTrigger>("flow_trigger", { name });
}

/**
 * Fa partire un flusso. Torna appena la corsa è avviata, non quando finisce:
 * un flusso che chiama un agente può durare mezz'ora, e il pulsante non deve
 * restare premuto per tutto quel tempo.
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
  state: "building" | "running" | "build_failed";
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

/** A step of a run that waits for a person, with what it asks. */
export interface HandedStep {
  step_id: string;
  /** Whom it was offered to: a label for the reader, not a credential. */
  holder: string;
  mandate: string;
  since: number;
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
 * Le corse che il guscio conosce.
 *
 * SERVE A CHI RICARICA LA PAGINA mentre un flusso gira: la corsa vive nel
 * guscio e continua, ma la tela ripartirebbe senza saperlo. Senza questo elenco
 * un lavoro in corso diventerebbe invisibile pur essendo vivo.
 */
export async function knownRuns(): Promise<RunSnapshot[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no runs to list");
  return invoke<RunSnapshot[]>("known_runs");
}

/**
 * Una corsa aperta, chiunque l'abbia avviata. Ricalca `OpenRun` di
 * `desktop/src-tauri/src/run.rs`: chi cambia l'uno cambia l'altro.
 */
export interface OpenRun {
  run_id: string;
  entity: string;
  /** «working» lavora, «waiting» è ferma e riparte solo se fai qualcosa. */
  state: "working" | "waiting";
  open_steps: number;
  /** **Quali** passi sono aperti, e da quanto. Vuoto per chi aspetta. */
  open_now: Array<{ step_id: string; attempt: number; open_for_secs: number }>;
  /** Da quando dura questo stato, in secondi dall'epoca. */
  since: number;
  /** Vero se questa finestra è quella che l'ha avviata. */
  started_here: boolean;
  /** Steps with an outcome already, counted once each. */
  steps_done: number;
  /** Steps the flow declares; null when the flow cannot be read back. */
  steps_total: number | null;
}

/**
 * Tutte le corse aperte sulla macchina, non solo quelle di questa finestra.
 *
 * **NON È `knownRuns` CON PIÙ RIGHE.** Quella legge la memoria del guscio e
 * conosce solo ciò che questa finestra ha avviato; questa interroga il
 * deposito, e vede anche una corsa partita dal terminale, da un'altra finestra
 * o da una pianificazione notturna. È la differenza fra una schermata che dice
 * «cosa sta succedendo» e una che dice «cosa ho fatto io» credendo siano la
 * stessa cosa.
 */
export async function openRuns(): Promise<OpenRun[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to ask");
  return invoke<OpenRun[]>("open_runs");
}

// ── quello che la plancia sapeva dire, e adesso lo dice la finestra ──────

/** Un riepilogo di giornata. Ricalca `DaySummary` di `board.rs`. */
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
  /** Chiamate al modello che non hanno riportato token. */
  unmeasured: number;
  /** Chiamate al modello che non hanno riportato un prezzo. */
  unpriced: number;
  tokens_by_model: Record<string, number>;
}

/**
 * Il riepilogo delle corse cominciate dopo un certo istante.
 *
 * **L'ISTANTE LO CALCOLA QUESTA FUNZIONE**, perché «oggi» è un giorno di
 * calendario locale e il fuso lo sa il sistema che disegna, non il motore. La
 * somma invece la fa il motore, una volta sola: due somme in due linguaggi
 * darebbero due cifre e nessuno saprebbe quale credere.
 */
export async function todaySummary(): Promise<DaySummary> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to sum up");
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  return invoke<DaySummary>("day_summary", { since: Math.floor(midnight.getTime() / 1000) });
}

/** Una corsa nella storia. Ricalca `ExecutionView` di `crates/ui/src/dashboard.rs`. */
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
  /** Token visti per modello, gia' sommati dal motore. */
  tokens_by_model: Record<string, { input_tokens: number; output_tokens: number; cached_tokens: number; cache_write_tokens: number; cost_micros: number; calls: number }>;
  calls: ModelCall[];
}

/**
 * Una chiamata al modello dentro una corsa.
 *
 * **`declared_cost_micros` NON E' UN DOPPIONE DI `cost_micros`.** Uno e' il
 * prezzo che Sailor calcola dai token, l'altro e' quello che il motore dice di
 * aver speso. Tenerli affiancati e' l'unico modo di accorgersi che uno dei due
 * ha torto — ed e' esattamente il difetto che nella ricognizione del 31/08/2026
 * e' rimasto invisibile a Langfuse, LangSmith e Phoenix, tutti e tre con numeri
 * sbagliati mostrati con autorita'.
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
  /** Quanto il motore dice di aver speso, quando lo dice. */
  declared_cost_micros: number | null;
  error_type: string | null;
  started_at: number;
  ended_at: number | null;
}

/** Tutte le corse che il deposito ricorda, dalla più recente. */
export async function executionHistory(): Promise<Execution[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no history to read");
  return invoke<Execution[]>("execution_history");
}

/** Una cosa installata su questa macchina. Ricalca `Entry` di `crates/inventory`. */
export interface InstalledEntry {
  kind: "skill" | "agent" | "command" | "rule" | "hook";
  name: string;
  description: string;
  /** Da dove viene: `casa`, `plugin <nome>`, `repo <nome>`. */
  origin: string;
  path: string;
  reach: { state: "active" } | { state: "inactive"; reason: string } | { state: "unknown"; reason: string };
  /** Il modello la può invocare da sé, o solo la persona che digita. */
  by_model: boolean;
}

export interface Installed {
  entries: InstalledEntry[];
  /** Dove ha guardato davvero: un elenco che non lo dice non si può smentire. */
  roots: string[];
  stale_plugin_copies: number;
}

/** Competenze, agenti, comandi, regole, ganci: cosa c'è su questa macchina. */
export async function machineInventory(): Promise<Installed> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: the engine takes the census");
  return invoke<Installed>("machine_inventory");
}

/** Un comando della riga di comando, come lo dichiara il binario. */
export interface CommandDoc {
  /** Il nome che si digita: `flow`, `step`, `release`. */
  name: string;
  /** La riga che dice a cosa serve — la stessa di `sailor --help`. */
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
 * I comandi che questo Sailor sa eseguire.
 *
 * **NON C'È NESSUN ELENCO DI COMANDI IN TYPESCRIPT, ED È IL PUNTO.** Scriverli
 * qui sarebbe stata mezz'ora di lavoro e una pagina che diverge dal binario
 * alla prima opzione aggiunta: il guasto 10, che in questo repo si è già
 * ripresentato cinque volte — l'ultima lo stesso giorno, sul vocabolario delle
 * azioni. `crates/sailor` è lib+bin apposta, e `manual` traduce soltanto la
 * forma.
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
 * Si mette in ascolto di quello che succede nelle corse.
 *
 * **UN ASCOLTO CHE NON SI ATTACCA DEVE DIRLO.** Tornare `null` in silenzio è
 * quello che questa funzione faceva prima, e il 28/08/2026 è costato una vista
 * che mostrava «in corso da 00:30» su una corsa già finita da un pezzo: il
 * canale non c'era, nessuno lo sapeva, e la finestra continuava a disegnare
 * l'ultimo stato che aveva ricevuto. Chi chiama deve poter dire a chi guarda
 * che quello che vede non si aggiorna da solo.
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

// ── cosa è entrato in un nodo, nel tempo ─────────────────────────────────

/**
 * Una volta in cui un passo è stato attraversato.
 *
 * Viene dal deposito, non dalla memoria di questa finestra: le corse che questa
 * finestra ha avviato sono una manciata, quelle che il nodo ha visto passare
 * possono essere centinaia — avviate dalla riga di comando, da una
 * pianificazione, o da una finestra chiusa mesi fa.
 */
export interface StepPassage {
  run_id: string;
  attempt: number;
  started_at: number;
  ended_at: number | null;
  outcome: string | null;
  failure_class: string | null;
  /** Da dove è partita la corsa: la provenienza, scritta dal sistema. */
  started_by: string;
  /** Che cosa è entrato in questo nodo, quella volta. */
  input: unknown;
  /** La consegna con cui è partita la corsa, se ne portava una. */
  mandate: string | null;
  /** Chi ha mandato il segnale, per come la sorgente lo sapeva. */
  signal_who: string | null;
  /** Da dove è arrivato: la finestra, un pannello, una sessione. */
  signal_where: string | null;
  said: string | null;
  output: unknown;
}

/** Tutto quello che è passato per un nodo, dal più recente. */
export async function stepHistory(flow: string, step: string, limit?: number): Promise<StepPassage[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the native shell: no ledger to ask");
  return invoke<StepPassage[]>("step_history", { flow, step, limit: limit ?? null });
}

/**
 * Quanto ha consumato una corsa: token, cache e denaro.
 *
 * I CONTI NON SI RIFANNO QUI. Li fa il motore (`ui::dashboard`), che è lo stesso
 * codice che serve la pagina di `sailor ui`: due somme scritte in due linguaggi
 * darebbero due cifre, e nessuno saprebbe quale credere.
 *
 * `null` non è un errore: è una corsa che il deposito non ha ancora proiettato,
 * o un deposito che non esiste perché non è mai stato eseguito niente.
 */
export async function runUsage(runId: string): Promise<RunUsage | null> {
  const invoke = invoker();
  if (!invoke) return null;
  return invoke<RunUsage | null>("run_usage", { runId });
}
