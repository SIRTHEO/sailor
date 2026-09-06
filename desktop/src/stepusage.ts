import type { CallView, RunUsage } from "./flow";
import { nodeId } from "./layout";

/**
 * What a single step cost, and with which engine it did it.
 *
 * **Why it exists.** `RunUsage.calls` already arrives from the store on every run
 * watched, carrying for each call the model really used, the tokens and the cost —
 * and it ended up crushed into one total line at the foot of the console. Whoever
 * watched a flow could not know which step had spent, nor on which model: exactly
 * the opacity the constraint «clarity for whoever is watching» forbids.
 *
 * It sits in a file of its own, without React, so the reckoning can be tested
 * without mounting a canvas — the same reason `runstate.ts` was split off.
 */

export interface StepUsage {
  /**
   * The models **really** used, in order of first call.
   *
   * A list and not a string because a step that retries can land on a model other
   * than the first, and showing one alone would lie about what happened. The model
   * declared in the step is elsewhere: this is what the engine answered it used.
   */
  models: string[];
  inputTokens: number;
  outputTokens: number;
  /**
   * `null` when **no** call of the step declared a cost.
   *
   * Not zero: zero would mean «it ran for free», and the difference is written
   * among the decisions — Codex declares the token total, not the two sides, so its
   * line stays costless. `0,0000 $` where nobody measured is an invented measure.
   */
  costMicros: number | null;
  calls: number;
  /** How many calls stayed out of the cost reckoning. */
  callsWithoutCost: number;
}

/** A null sum, for a step that has called nobody yet. */
function empty(): StepUsage {
  return { models: [], inputTokens: 0, outputTokens: 0, costMicros: null, calls: 0, callsWithoutCost: 0 };
}

function fold(into: StepUsage, call: CallView): StepUsage {
  const models = into.models.includes(call.actual_model) || call.actual_model === ""
    ? into.models
    : [...into.models, call.actual_model];
  // An absent cost leaves `null` until a real one arrives: summing `null` as
  // zero is exactly how a partial total disguises itself as a total.
  const cost = call.cost_micros === null
    ? into.costMicros
    : (into.costMicros ?? 0) + call.cost_micros;
  return {
    models,
    inputTokens: into.inputTokens + (call.input_tokens ?? 0),
    outputTokens: into.outputTokens + (call.output_tokens ?? 0),
    costMicros: cost,
    calls: into.calls + 1,
    callsWithoutCost: into.callsWithoutCost + (call.cost_micros === null ? 1 : 0),
  };
}

/**
 * A run's calls, gathered per step and keyed `flusso::passo`.
 *
 * The key is qualified with the flow for the reason already paid on the single
 * canvas: `verifica` and `verdetto` exist in more than one flow, and a bare key
 * would show on one node the spend of another.
 *
 * Calls with no `step_id` — the run's, not a step's — stay out: they belong to
 * the total, not to a node.
 */
export function stepUsageOfRun(usage: RunUsage | null, flowName: string): Map<string, StepUsage> {
  const perStep = new Map<string, StepUsage>();
  if (!usage) return perStep;
  for (const call of usage.calls) {
    if (call.step_id === null || call.step_id === "") continue;
    const key = nodeId(flowName, call.step_id);
    perStep.set(key, fold(perStep.get(key) ?? empty(), call));
  }
  return perStep;
}

/** True when the cost shown is lower than the real one, and must be said. */
export function usageIsPartial(usage: StepUsage): boolean {
  return usage.callsWithoutCost > 0;
}

/**
 * The cost, with an Italian decimal comma and the unit attached.
 *
 * Four decimals because one call often costs less than a cent, and rounding to
 * two would make it read `0,00 $` — the same deceit as an absent cost shown as zero.
 */
export function formatCost(micros: number): string {
  return `$${(micros / 1e6).toFixed(4)}`;
}

/**
 * The tokens in short form: `840`, `12.4k`, `1.03M`.
 *
 * On a node there is no room for seven digits, and the exact number is not the
 * question you ask looking at a canvas — the order of magnitude is.
 */
export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1e6) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1e6).toFixed(2)}M`;
}
