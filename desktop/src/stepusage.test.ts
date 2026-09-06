import { describe, expect, test } from "vitest";
import type { CallView, RunUsage } from "./flow";
import { stepUsageOfRun, usageIsPartial } from "./stepusage";

/**
 * **THE SPEND IS ATTRIBUTED TO THE STEP THAT MADE IT.**
 *
 * The calls arrived from the ledger and ended in a single total: nobody could
 * know which step had spent, nor on which model. These tests pin the three
 * things that, got wrong, would say the false thing on the face of a node: the
 * key qualified with the flow, the absent cost that does not become zero, and
 * the real model, which is the one the engine answered with.
 */

function call(over: Partial<CallView>): CallView {
  return {
    call_id: "c",
    step_id: "implementa",
    cli: "claude-code",
    actual_model: "claude-sonnet-4",
    input_tokens: 100,
    output_tokens: 20,
    cached_tokens: 0,
    cache_write_tokens: 0,
    cache_write_long_tokens: 0,
    total_tokens: 120,
    cost_micros: 1000,
    declared_cost_micros: null,
    error_type: null,
    started_at: 0,
    ended_at: 1,
    ...over,
  } as CallView;
}

function usageWith(calls: CallView[]): RunUsage {
  return { calls } as unknown as RunUsage;
}

describe("la spesa per passo", () => {
  test("senza corsa non inventa niente", () => {
    expect(stepUsageOfRun(null, "sviluppa-sailor").size).toBe(0);
  });

  test("la chiave porta il flusso, così la spesa non finisce sul nodo di un altro", () => {
    // `verifica` and `verdetto` exist in several real flows of this machine:
    // with a bare key the node would show the spend of a flow not its own.
    const perStep = stepUsageOfRun(usageWith([call({})]), "sviluppa-sailor");
    expect(perStep.get("sviluppa-sailor::implementa")?.calls).toBe(1);
    expect(perStep.get("implementa")).toBeUndefined();
  });

  test("due chiamate dello stesso passo si sommano", () => {
    const perStep = stepUsageOfRun(usageWith([call({}), call({ call_id: "d", cost_micros: 500 })]), "f");
    const found = perStep.get("f::implementa");
    expect(found?.calls).toBe(2);
    expect(found?.costMicros).toBe(1500);
    expect(found?.inputTokens).toBe(200);
  });

  test("UN COSTO ASSENTE RESTA ASSENTE, non diventa zero", () => {
    // Codex declares the token total and not the two sides, so its row stays
    // without a cost: showing `$0.0000` would be an invented measurement.
    const perStep = stepUsageOfRun(usageWith([call({ cost_micros: null })]), "f");
    const found = perStep.get("f::implementa");
    expect(found?.costMicros).toBe(null);
    expect(usageIsPartial(found!)).toBe(true);
  });

  test("una chiamata senza costo abbassa il totale, e il passo lo dichiara", () => {
    const perStep = stepUsageOfRun(usageWith([call({}), call({ call_id: "d", cost_micros: null })]), "f");
    const found = perStep.get("f::implementa");
    expect(found?.costMicros).toBe(1000);
    expect(found?.callsWithoutCost).toBe(1);
    expect(usageIsPartial(found!)).toBe(true);
  });

  test("il modello vero è quello che il motore ha risposto, e i ritentativi ne portano più d'uno", () => {
    const perStep = stepUsageOfRun(
      usageWith([call({}), call({ call_id: "d", actual_model: "claude-opus-4" })]),
      "f",
    );
    expect(perStep.get("f::implementa")?.models).toEqual(["claude-sonnet-4", "claude-opus-4"]);
  });

  test("le chiamate della corsa, che non sono di un passo, restano fuori dai nodi", () => {
    const perStep = stepUsageOfRun(usageWith([call({ step_id: null })]), "f");
    expect(perStep.size).toBe(0);
  });
});
