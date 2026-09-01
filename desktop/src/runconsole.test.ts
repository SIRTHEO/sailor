import { describe, expect, test } from "vitest";
import type { RunEvent } from "./engine";
import { linesFromEvents, panesFromEvents } from "./RunConsole";

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

function text(seq: number, stepId: string, pipe: string, said: string): RunEvent {
  return { run_id: "r", seq, kind: "step_text", at: seq * 1000, step_id: stepId, payload: { pipe, text: said } };
}

/**
 * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. An unknown kind falls to
 * the default arm, which reads `text` and calls every line a system line: the
 * text of a running step would arrive looking like something the shell said,
 * with an error indistinguishable from ordinary output.
 */
describe("what a step says while it runs", () => {
  test("it reaches the console under the pipe it came from", () => {
    const lines = linesFromEvents([
      started(1, "engine", { bin: "claude" }),
      text(2, "engine", "out", "reading the file\n"),
      text(3, "engine", "err", "cannot open it\n"),
    ]);
    const said = lines.filter((line) => line.stream !== "system");
    expect(said.map((line) => [line.stream, line.text])).toEqual([
      ["stdout", "reading the file"],
      ["stderr", "cannot open it"],
    ]);
  });

  test("a step that is still running has already spoken", () => {
    const panes = panesFromEvents([started(1, "engine", { bin: "claude" }), text(2, "engine", "out", "working\n")]);
    expect(panes[0]?.spoke).toBe(true);
    expect(panes[0]?.endedAt).toBe(null);
  });
});
