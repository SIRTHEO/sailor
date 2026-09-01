/**
 * What a step is, in the run that is happening — as opposed to what it says
 * about itself in the flow file. Kept out of the component because these are
 * the parts with a right and a wrong answer: a test can hand them a graph and
 * a record and look at what comes back, without mounting React.
 */
import type { Graph } from "./flow";
import type { Step } from "./flow";
import { PROMPT_KEY } from "./tools";

/** Where an engine reads its mandate when it is not given a prompt. */
const STDIN_KEY = "stdin";

/**
 * The mandate this step was given, as text.
 *
 * What it received wins over what it declared: a step can leave its prompt open
 * and be handed one when the run starts, and reading only the flow file would
 * show an empty mandate for exactly the steps someone wrote one for by hand.
 */
export function mandateOf(step: Step, received: unknown): string | null {
  const record = received && typeof received === "object" ? (received as Record<string, unknown>) : null;
  const declared = step.with ?? null;
  for (const source of [record, declared]) {
    if (!source) continue;
    for (const key of [PROMPT_KEY, STDIN_KEY]) {
      const value = source[key];
      if (typeof value === "string" && value.trim() !== "") return value;
    }
  }
  return null;
}

/** The steps this one waits for, and the ones that wait for it. */
export function neighboursOf(graph: Graph, stepId: string): { before: string[]; after: string[] } {
  const mine = graph.steps.find((step) => step.id === stepId);
  if (!mine) return { before: [], after: [] };
  return {
    before: [...mine.deps],
    after: graph.steps.filter((step) => step.deps.includes(stepId)).map((step) => step.id),
  };
}
