import { describe, expect, test } from "vitest";
import type { RunEvent } from "./engine";
import { panesFromEvents } from "./RunConsole";

/**
 * **COSA È ENTRATO IN UN PASSO ARRIVA FINO ALLA VISTA.**
 *
 * `step_started` portava `input` da sempre, e la console lo leggeva soltanto
 * per indovinare che azione fosse, poi lo buttava. Chi guardava una corsa
 * vedeva cosa ogni passo aveva **detto** e mai cosa gli era stato **dato**:
 * metà del vincolo «chiarezza per chi guarda», e la metà che spiega l'altra.
 *
 * Questa prova sta sul calcolo e non sul disegno perché è lì che il dato si
 * perdeva; il riquadro che lo mostra è tre righe di JSX sopra questo.
 */

function started(seq: number, stepId: string, input: unknown): RunEvent {
  return { run_id: "r", seq, kind: "step_started", at: seq * 1000, step_id: stepId, payload: { attempt: 1, input } };
}

function closed(seq: number, stepId: string, outcome: string): RunEvent {
  return { run_id: "r", seq, kind: "step_closed", at: seq * 1000, step_id: stepId, payload: { outcome } };
}

describe("i riquadri di una corsa", () => {
  test("un riquadro porta cosa è entrato nel passo", () => {
    const panes = panesFromEvents([
      started(1, "implementa", { tool: "claude-code", prompt: "ripara il nodo" }),
      closed(2, "implementa", "Went"),
    ]);
    expect(panes[0]?.input).toEqual({ tool: "claude-code", prompt: "ripara il nodo" });
  });

  test("un passo senza input non porta un oggetto vuoto, che sembrerebbe un input", () => {
    // `null` è «non è entrato niente di registrato»; `{}` sarebbe «è entrato un
    // record vuoto», e sono due fatti diversi per chi legge una corsa.
    const panes = panesFromEvents([started(1, "verifica", undefined), closed(2, "verifica", "Went")]);
    expect(panes[0]?.input).toBe(null);
  });

  test("ogni passo tiene il proprio input, non quello del vicino", () => {
    const panes = panesFromEvents([
      started(1, "uno", { command: "cargo test" }),
      started(2, "due", { command: "cargo fmt" }),
      closed(3, "uno", "Went"),
      closed(4, "due", "Broke"),
    ]);
    const byId = new Map(panes.map((p) => [p.stepId, p]));
    expect(byId.get("uno")?.input).toEqual({ command: "cargo test" });
    expect(byId.get("due")?.input).toEqual({ command: "cargo fmt" });
  });
});
