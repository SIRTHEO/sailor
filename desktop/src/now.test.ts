import { describe, expect, test } from "vitest";
import { groupRuns, howLong } from "./Now";
import type { OpenRun } from "./engine";

/**
 * **LE DUE COSE CHE LA PRIMA SCHERMATA PUÒ SBAGLIARE IN SILENZIO.**
 *
 * Una durata scritta male e un raggruppamento che perde una riga non fanno
 * rumore: la schermata si disegna lo stesso, e chi guarda crede a quello che
 * legge. Sono esattamente i due difetti che la ricognizione del 31/08/2026 ha
 * trovato pubblici e confermati negli altri prodotti — un badge che conta 1
 * mentre la lista ne mostra dieci, una latenza di 9,3 secondi scritta «9.3K».
 */

function run(over: Partial<OpenRun>): OpenRun {
  return {
    run_id: "r1",
    entity: "sviluppa-sailor",
    state: "working",
    open_steps: 1,
    open_now: [],
    since: 0,
    started_here: false,
    steps_done: 0,
    steps_total: null,
    ...over,
  };
}

describe("howLong", () => {
  test("arrotonda per difetto, non per eccesso", () => {
    // Due ore e cinquanta: «3 h» farebbe intervenire su una cosa che non è
    // ancora quello che sembra.
    expect(howLong(2 * 3600 + 50 * 60)).toBe("2 h 50 min");
    expect(howLong(119)).toBe("1 min");
  });

  test("i secondi restano secondi finché sono secondi", () => {
    expect(howLong(0)).toBe("0 s");
    expect(howLong(59)).toBe("59 s");
    expect(howLong(60)).toBe("1 min");
  });

  test("oltre il giorno non si scrive in ore", () => {
    // 50 h scritte «50 h» si leggono come due giorni solo contando: chi guarda
    // deve vedere che una corsa è aperta da ieri senza fare aritmetica.
    expect(howLong(50 * 3600)).toBe("2 d 2 h");
  });

  test("un tempo negativo non diventa un numero enorme", () => {
    // Gli orologi non sono d'accordo fra loro: il deposito scrive un istante,
    // la finestra ne legge un altro, e la differenza può uscire negativa. Un
    // «-3 s» è brutto, ma «18446744073709 s» è una schermata rotta.
    expect(howLong(-3)).toBe("—");
  });
});

describe("groupRuns", () => {
  test("nessuna corsa si perde per strada", () => {
    const runs = [
      run({ run_id: "a", state: "waiting" }),
      run({ run_id: "b", state: "working" }),
      run({ run_id: "c", state: "waiting" }),
    ];
    const { waiting, working } = groupRuns(runs);
    expect(waiting.map((entry) => entry.run_id)).toEqual(["a", "c"]);
    expect(working.map((entry) => entry.run_id)).toEqual(["b"]);
    expect(waiting.length + working.length).toBe(runs.length);
  });

  test("l'ordine che arriva dal motore non si tocca", () => {
    // Il motore ordina dalla più vecchia. Riordinare qui vorrebbe dire avere
    // due regole d'ordine in due linguaggi, e nessuno saprebbe quale vince.
    const runs = [
      run({ run_id: "vecchia", state: "waiting", since: 100 }),
      run({ run_id: "nuova", state: "waiting", since: 900 }),
    ];
    expect(groupRuns(runs).waiting.map((entry) => entry.run_id)).toEqual(["vecchia", "nuova"]);
  });
});
