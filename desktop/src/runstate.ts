import type { RunEvent, RunSnapshot } from "./engine";
import type { StepRun, StepState } from "./flow";
import { nodeId } from "./layout";

/**
 * Da fatti a stato per nodo.
 *
 * **PERCHÉ ESISTE.** Fino al commit che porta questo file la tela diceva «in
 * attesa» su ogni nodo di ogni flusso vero, anche mentre il motore lavorava:
 * il disegno riceveva una mappa vuota, e `executions` — i fatti veri, che
 * arrivano già per evento — non era dipendenza di niente. È la violazione più
 * netta possibile del vincolo permanente «un'interfaccia che nasconde cosa
 * succede è il contrario del prodotto»: non nascondeva, raccontava il falso.
 *
 * **STA FUORI DA REACT** per la stessa ragione di `linesFromEvents`: è la sola
 * parte con una risposta giusta e una sbagliata, e una prova può darle degli
 * eventi e guardare cosa produce senza montare niente.
 */

/// Gli esiti che il deposito scrive, tradotti negli stati che un nodo sa
/// disegnare. Ciò che non è qui dentro resta `undefined` e il nodo torna a
/// «in attesa»: uno stato indovinato su un esito che non conosciamo sarebbe
/// esattamente la bugia che questo file esiste per togliere.
const STATE_OF_OUTCOME: Record<string, StepState> = {
  Went: "went",
  Broke: "broke",
  Stopped: "capped",
  Waiting: "waiting",
  // A step that answered «not yet, ask me on the next beat» is closed and
  // will be asked again: it waits, it is not still running.
  NotYet: "waiting",
  // A branch nobody took. Not waiting: nothing will make it run.
  Skipped: "skipped",
};

/**
 * Lo stato di ogni passo di **una** corsa, letto dai suoi eventi.
 *
 * Un passo che è partito e non ha ancora chiuso sta correndo; uno che ha chiuso
 * porta l'esito della chiusura. Gli eventi si leggono in ordine di `seq`, non
 * di arrivo: un tentativo che ritorna tardi non deve riscrivere lo stato di
 * quello che l'ha superato.
 */
export function stepStatesOfRun(events: RunEvent[]): Map<string, StepRun> {
  const states = new Map<string, StepRun>();
  const startedAt = new Map<string, number>();
  const ordered = [...events].sort((a, b) => a.seq - b.seq);

  for (const event of ordered) {
    const stepId = event.step_id;
    if (stepId === null) continue;
    const payload = event.payload as Record<string, unknown> | null;

    if (event.kind === "step_started") {
      const attempt = typeof payload?.attempt === "number" ? payload.attempt : 1;
      const heldBy = typeof payload?.held_by_pid === "number" ? payload.held_by_pid : undefined;
      states.set(stepId, { step_id: stepId, state: "running", attempt, held_by_pid: heldBy });
      // WHEN THIS ATTEMPT BEGAN, kept aside rather than in the state: a retry
      // measured from the first start would report the wait between them too.
      startedAt.set(stepId, event.at);
      continue;
    }

    // WHAT IT IS SAYING WHILE IT RUNS. The engine has always sent this piece by
    // piece and nothing read it: on the canvas a step that had just printed
    // thirty lines and one stuck for eight minutes were drawn the same.
    if (event.kind === "step_text") {
      const current = states.get(stepId);
      if (current) states.set(stepId, { ...current, spoke_at: event.at });
      continue;
    }

    if (event.kind === "step_closed") {
      const outcome = typeof payload?.outcome === "string" ? payload.outcome : "";
      // LA SPECIE VINCE SULL'ESITO quando dice che il passo aspetta una
      // persona: `hand_to_human` è un rotto che nessuno ritenterà, e il tipo
      // `StepState` li tiene distinti apposta — mostrarlo come un guasto
      // qualunque manderebbe chi guarda a cercare un difetto invece di
      // rispondere.
      const species = typeof payload?.species === "string" ? payload.species : "";
      const previous = states.get(stepId);
      const began = startedAt.get(stepId);
      const state: StepState | undefined =
        outcome === "Broke" && species === "hand_to_human"
          ? "handed_to_human"
          : STATE_OF_OUTCOME[outcome];
      if (state === undefined) continue;
      states.set(stepId, {
        step_id: stepId,
        state,
        attempt: previous?.attempt ?? 1,
        // The two instants were in the events all along, and nothing read
        // them: the cell that shows this said «—» on every run there has
        // ever been.
        elapsed_secs: began === undefined ? previous?.elapsed_secs : event.at - began,
      });
    }
  }

  return states;
}

/**
 * Lo stato di ogni nodo della tela, da tutte le corse conosciute.
 *
 * **LA CHIAVE È IL NOME QUALIFICATO `flusso::passo`, non l'identificativo nudo.**
 * Sulla tela unificata i flussi stanno insieme, e fra quelli veri su questa
 * macchina tre identificativi sono già ripetuti — `trigger`, `verifica`,
 * `verdetto`. Con la chiave nuda lo stato di un flusso colorerebbe il nodo
 * omonimo di un altro, che è peggio del grigio di prima: un errore che si
 * legge come una misura.
 *
 * Quando due corse dello stesso flusso sono note vince la più recente: è quella
 * che chi guarda ha appena lanciato.
 */
export function stepStatesOfCanvas(runs: Iterable<RunSnapshot>): Map<string, StepRun> {
  const newest = new Map<string, RunSnapshot>();
  for (const run of runs) {
    const seen = newest.get(run.flow);
    if (!seen || run.started_at >= seen.started_at) newest.set(run.flow, run);
  }

  const states = new Map<string, StepRun>();
  for (const run of newest.values()) {
    for (const [stepId, state] of stepStatesOfRun(run.events)) {
      states.set(nodeId(run.flow, stepId), state);
    }
  }
  return states;
}
