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
