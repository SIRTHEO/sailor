import { describe, expect, test } from "vitest";
import type { RunEvent } from "./engine";
import { linesFromEvents, panesFromEvents, stopRequested } from "./RunConsole";

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

function closed(seq: number, stepId: string, outcome: string, output?: unknown): RunEvent {
  return { run_id: "r", seq, kind: "step_closed", at: seq * 1000, step_id: stepId, payload: { outcome, output } };
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

/**
 * A stop asked by hand is a fact of the run: the console shows it as a line,
 * honest about what it can do, and the status word changes while the running
 * step finishes. Once the run has ended, nothing is "stopping" any more.
 */
describe("a stop asked by hand", () => {
  const stop: RunEvent = { run_id: "r", seq: 2, kind: "stop_requested", at: 10, step_id: null, payload: { by: "theo" } };

  test("is a line of the console that says the running step finishes", () => {
    const lines = linesFromEvents([started(1, "engine", { bin: "claude" }), stop]);
    const said = lines.find((line) => line.key.endsWith(":stop"));
    expect(said?.stream).toBe("system");
    expect(said?.text).toContain("the one running finishes");
  });

  test("counts as stopping only while the run is still running", () => {
    const events = [started(1, "engine", { bin: "claude" }), stop];
    expect(stopRequested({ run_id: "r", flow: "f", started_at: 0, status: "running", events })).toBe(true);
    expect(stopRequested({ run_id: "r", flow: "f", started_at: 0, status: "stopped", events })).toBe(false);
    expect(stopRequested({ run_id: "r", flow: "f", started_at: 0, status: "running", events: [events[0]] })).toBe(false);
  });
});

/**
 * What came out was read once, turned into lines, and dropped. A panel that
 * shows what went in and not what came out shows half of a step, and it is the
 * half that answers nothing.
 */
describe("what came out of a step", () => {
  test("a pane keeps the output, not only the lines made from it", () => {
    const panes = panesFromEvents([
      started(1, "read", { bin: "cat" }),
      closed(2, "read", "Went", { stdout: "four lines", status: 0 }),
    ]);
    expect(panes[0]?.output).toEqual({ stdout: "four lines", status: 0 });
  });

  test("a step still running has no output, which is not an empty one", () => {
    const panes = panesFromEvents([started(1, "read", { bin: "cat" })]);
    expect(panes[0]?.output).toBe(null);
  });
});
