import { describe, expect, test } from "vitest";
import { lastedOf, outcomeOf, whenOf } from "./History";
import { countByFamily } from "./Installed";
import type { Execution, InstalledEntry } from "./engine";

/**
 * **LE REGOLE CHE DECIDONO IL COLORE DI UNA RIGA.**
 *
 * Sbagliarle non fa cadere niente: la tabella si disegna lo stesso, e chi legge
 * crede a quello che vede. Una corsa rotta contata fra le aperte esce
 * dall'occhio di chi cerca i guasti, ed è il difetto più caro possibile in una
 * vista che esiste per trovarli.
 */

function run(over: Partial<Execution>): Execution {
  return {
    run_id: "r",
    kind: "flow",
    entity: "sviluppa-sailor",
    status: "succeeded",
    started_at: 0,
    ended_at: 1,
    duration_secs: 1,
    total_cost_micros: 0,
    error: null,
    steps_total: 3,
    steps_went: 3,
    steps_broke: 0,
    steps_retried: 0,
    steps_open: [],
    tokens: {
      input_tokens: 0,
      output_tokens: 0,
      cached_tokens: 0,
      cache_write_tokens: 0,
      cost_micros: 0,
      calls: 0,
      calls_without_tokens: 0,
      calls_without_cost: 0,
    },
    ...over,
  };
}

describe("outcomeOf", () => {
  test("ROTTA VINCE SU APERTA", () => {
    // Una corsa con un passo caduto e un altro ancora in volo è un guasto che
    // sta ancora bruciando. Contarla fra le aperte la toglierebbe dall'occhio
    // di chi guarda i guasti — ed è la riga che questa prova difende.
    const both = run({
      status: "running",
      steps_broke: 1,
      steps_open: [{ step_id: "prove", attempt: 1, started_at: 0, open_for_secs: 30 }],
    });
    expect(outcomeOf(both)).toBe("broke");
  });

  test("un errore scritto basta, anche senza passi caduti", () => {
    expect(outcomeOf(run({ error: "il deposito non risponde", steps_broke: 0 }))).toBe("broke");
  });

  test("uno stato che non conosciamo non diventa «andata»", () => {
    // «altro» è brutto da leggere e onesto: inventare un successo su uno stato
    // che il motore non ha dichiarato è il modo in cui una vista comincia a
    // mentire senza che nessuno la smentisca.
    expect(outcomeOf(run({ status: "waiting", steps_open: [] }))).toBe("other");
  });

  test("i tentativi non cambiano l'esito", () => {
    // Un passo ripetuto e poi riuscito è una corsa andata, e la fatica si legge
    // nella colonna dei ritentati invece che in un rosso che non c'è.
    expect(outcomeOf(run({ steps_retried: 2 }))).toBe("went");
  });
});

describe("lastedOf", () => {
  test("una corsa mai finita non dura zero", () => {
    // `0 s` su una corsa che non è mai finita è la bugia comoda: sembra
    // istantanea invece che interrotta.
    expect(lastedOf(null)).toBe("—");
  });

  test("le durate salgono di scala invece di allungarsi", () => {
    expect(lastedOf(45)).toBe("45 s");
    expect(lastedOf(90)).toBe("1 min");
    expect(lastedOf(3 * 3600 + 25 * 60)).toBe("3 h 25 min");
  });
});

describe("whenOf", () => {
  test("oggi si scrive solo l'ora, un altro giorno porta la data", () => {
    const now = new Date(2026, 7, 31, 21, 0, 0).getTime() / 1000;
    const thisMorning = new Date(2026, 7, 31, 9, 5, 0).getTime() / 1000;
    const yesterday = new Date(2026, 7, 30, 9, 5, 0).getTime() / 1000;
    expect(whenOf(thisMorning, now)).toBe("09:05");
    expect(whenOf(yesterday, now)).toBe("30/08 09:05");
  });
});

describe("countByFamily", () => {
  test("le famiglie a zero restano nell'elenco", () => {
    // Una famiglia che sparisce quando è vuota fa credere che non esista: chi
    // non ha ancora scritto un gancio deve vedere «ganci 0», non il silenzio.
    const entries: InstalledEntry[] = [
      { kind: "skill", name: "a", description: "", origin: "casa", path: "/a", reach: { state: "active" }, by_model: true },
      { kind: "skill", name: "b", description: "", origin: "casa", path: "/b", reach: { state: "active" }, by_model: true },
      { kind: "hook", name: "c", description: "", origin: "casa", path: "/c", reach: { state: "active" }, by_model: false },
    ];
    expect(countByFamily(entries)).toEqual({ skill: 2, agent: 0, command: 0, rule: 0, hook: 1 });
  });
});
