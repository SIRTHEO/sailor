import { describe, expect, test } from "vitest";
import type { RunEvent, RunSnapshot } from "./engine";
import { stepStatesOfCanvas, stepStatesOfRun } from "./runstate";

/**
 * Le prime prove che questa finestra abbia mai avuto, e provano la sola parte
 * che ha una risposta giusta e una sbagliata: da eventi a stato per nodo.
 *
 * Ciascuna può venire diversa, e si rompe rompendo ciò che afferma:
 * togliere l'ordinamento per `seq`, togliere la chiave qualificata, togliere la
 * precedenza della specie — e diventano rosse una per una.
 */

function started(seq: number, stepId: string, extra: Record<string, unknown> = {}): RunEvent {
  return { run_id: "r", seq, kind: "step_started", at: seq, step_id: stepId, payload: { attempt: 1, ...extra } };
}

function closed(seq: number, stepId: string, outcome: string, extra: Record<string, unknown> = {}): RunEvent {
  return { run_id: "r", seq, kind: "step_closed", at: seq, step_id: stepId, payload: { outcome, ...extra } };
}

function snapshot(flow: string, startedAt: number, events: RunEvent[]): RunSnapshot {
  return { run_id: `${flow}-${startedAt}`, flow, started_at: startedAt, status: "running", events };
}

describe("lo stato di una corsa", () => {
  test("un passo partito e non ancora chiuso sta correndo", () => {
    const states = stepStatesOfRun([started(1, "piano")]);
    expect(states.get("piano")?.state).toBe("running");
  });

  test("un passo chiuso porta l'esito, non più «in corso»", () => {
    const states = stepStatesOfRun([started(1, "piano"), closed(2, "piano", "Went")]);
    expect(states.get("piano")?.state).toBe("went");
  });

  test("i quattro finali restano distinti, perché non sono intercambiabili", () => {
    const states = stepStatesOfRun([
      closed(1, "a", "Went"),
      closed(2, "b", "Broke"),
      closed(3, "c", "Stopped"),
      closed(4, "d", "Broke", { species: "hand_to_human" }),
    ]);
    expect(states.get("a")?.state).toBe("went");
    expect(states.get("b")?.state).toBe("broke");
    expect(states.get("c")?.state).toBe("capped");
    // «Aspetta una persona» è un rotto che nessuno ritenterà: mostrarlo rosso
    // manda chi guarda a cercare un difetto invece di rispondere.
    expect(states.get("d")?.state).toBe("handed_to_human");
  });

  test("un esito che non conosciamo lascia il passo com'era, non lo indovina", () => {
    const states = stepStatesOfRun([started(1, "x"), closed(2, "x", "Qualcosa_Di_Nuovo")]);
    expect(states.get("x")?.state).toBe("running");
  });

  test("gli eventi si leggono in ordine di seq, non di arrivo", () => {
    // Un tentativo che ritorna tardi non deve riscrivere lo stato di quello che
    // l'ha superato: qui la chiusura (seq 2) arriva DOPO la ripartenza (seq 3).
    const states = stepStatesOfRun([started(1, "x"), started(3, "x", { attempt: 2 }), closed(2, "x", "Broke")]);
    expect(states.get("x")?.state).toBe("running");
    expect(states.get("x")?.attempt).toBe(2);
  });
});

describe("lo stato della tela intera", () => {
  test("la chiave è «flusso::passo»: due flussi con lo stesso id non si contaminano", () => {
    // È il caso vero: fra i flussi su questa macchina `verifica`, `trigger` e
    // `verdetto` sono ripetuti. Con la chiave nuda lo stato di uno colorerebbe
    // il nodo omonimo dell'altro — un errore che si legge come una misura.
    const states = stepStatesOfCanvas([
      snapshot("primo", 10, [closed(1, "verifica", "Went")]),
      snapshot("secondo", 11, [closed(1, "verifica", "Broke")]),
    ]);
    expect(states.get("primo::verifica")?.state).toBe("went");
    expect(states.get("secondo::verifica")?.state).toBe("broke");
    expect(states.get("verifica")).toBeUndefined();
  });

  test("di due corse dello stesso flusso conta la più recente", () => {
    const states = stepStatesOfCanvas([
      snapshot("f", 100, [closed(1, "p", "Broke")]),
      snapshot("f", 200, [started(1, "p")]),
    ]);
    expect(states.get("f::p")?.state).toBe("running");
  });

  test("senza corse nessun nodo ha uno stato, e nessuno ne inventa uno", () => {
    expect(stepStatesOfCanvas([]).size).toBe(0);
  });
});

/**
 * THE ONE NUMBER THE NODE HAD A SLOT FOR AND NEVER A VALUE. `elapsed_secs` was
 * carried forward and nothing ever assigned it, so the cell read `—` on every
 * run there has ever been — while both instants sat in the events.
 */
describe("how long a step took", () => {
  function at(seq: number, kind: string, stepId: string, at: number, payload: unknown = {}): RunEvent {
    return { run_id: "r", seq, kind: kind as RunEvent["kind"], at, step_id: stepId, payload };
  }

  test("it is the distance between the two instants the events carry", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1007, { outcome: "Went" }),
    ]);
    expect(states.get("read")?.elapsed_secs).toBe(7);
  });

  /* A step still running has no duration yet, and zero is not «no duration»:
     it is «it took no time», which is a different fact. */
  test("a step that has not closed has none", () => {
    const states = stepStatesOfRun([at(1, "step_started", "read", 1000, { attempt: 1 })]);
    expect(states.get("read")?.elapsed_secs).toBeUndefined();
  });

  /* A retry is a second run of the same step, and its duration is its own:
     measuring from the first attempt would report the wait between them too. */
  test("a retry is measured from its own start", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1002, { outcome: "Broke" }),
      at(3, "step_started", "read", 1060, { attempt: 2 }),
      at(4, "step_closed", "read", 1063, { outcome: "Went" }),
    ]);
    expect(states.get("read")?.elapsed_secs).toBe(3);
  });

  /* THE THIRD OUTCOME IS KNOWN TO THE WINDOW. A step that answered «not yet»
     is closed and will be asked again on the next beat: drawn as still
     running, it would look held by a process that is not there. */
  test("a step that said not yet is waiting, not running", () => {
    const states = stepStatesOfRun([
      at(1, "step_started", "read", 1000, { attempt: 1 }),
      at(2, "step_closed", "read", 1001, { outcome: "NotYet" }),
    ]);
    expect(states.get("read")?.state).toBe("waiting");
  });
})
