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

import type { FlowEntry, FlowFile, RunUsage } from "./flow";
import { parseTools, publishTools, type Tool } from "./tools";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

interface TauriGlobal {
  core?: { invoke?: Invoke };
}

function invoker(): Invoke | null {
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore da interrogare");
  return invoke<FlowEntry[]>("flows");
}

/**
 * Scrive un flusso sul disco, tramite il motore. `save_flow` nasce insieme a
 * questo pannello, in un altro cantiere sullo stesso guscio: finché quel lato
 * non risponde, questa chiamata fallisce con un errore leggibile invece di
 * restare muta.
 */
export async function saveFlow(flow: FlowFile): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore a cui salvare");
  await invoke<void>("save_flow", { flow });
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore a cui chiedere gli strumenti");
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore da cui cancellare");
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
  kind: "step_started" | "step_closed" | "run_ended" | "note";
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore da innescare");
  return invoke<FlowTrigger>("flow_trigger", { name });
}

/**
 * Fa partire un flusso. Torna appena la corsa è avviata, non quando finisce:
 * un flusso che chiama un agente può durare mezz'ora, e il pulsante non deve
 * restare premuto per tutto quel tempo.
 */
export async function startRun(name: string, mandate: string | null): Promise<StartedRun> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore che esegua");
  return invoke<StartedRun>("start_run", { name, mandate });
}

/** Tutto quello che una corsa ha detto finora, per chi si affaccia adesso. */
export async function runSnapshot(runId: string): Promise<RunSnapshot> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessuna corsa da guardare");
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessuna corsa da elencare");
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
  /** Da quando dura questo stato, in secondi dall'epoca. */
  since: number;
  /** Vero se questa finestra è quella che l'ha avviata. */
  started_here: boolean;
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun deposito da interrogare");
  return invoke<OpenRun[]>("open_runs");
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
  if (!tauri) return { why: "fuori dal guscio nativo: nessun canale di eventi" };
  const listen = tauri.event?.listen;
  if (!listen) {
    return {
      why: "il guscio non espone «event.listen»: la vista si aggiorna interrogando invece che in ascolto",
    };
  }
  try {
    const stop = await listen<RunEvent>("sailor://run", (event) => handler(event.payload));
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
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun deposito da interrogare");
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
