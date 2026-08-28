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

import type { FlowEntry, FlowFile } from "./flow";

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

/** Cancella un flusso dal disco, tramite il motore. Stessa premessa di `saveFlow`. */
export async function deleteFlow(name: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("fuori dal guscio nativo: nessun motore da cui cancellare");
  await invoke<void>("delete_flow", { name });
}
