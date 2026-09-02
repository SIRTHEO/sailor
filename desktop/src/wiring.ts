/**
 * A step born already wired to the one it came from. Creating and connecting in
 * sequence would read a stale graph — the second would not find what the first
 * just made — so here they are one edit, outside the component where a test can
 * hand them a graph and look at what comes back.
 */
import { DEFAULT_ACTION_FOR_KIND, type Graph, type Step, type StepKind } from "./flow";

/** The first name of this family nobody is using. */
export function freeStepId(graph: Graph, kind: StepKind): string {
  const taken = new Set(graph.steps.map((step) => step.id));
  let n = 1;
  while (taken.has(`${kind}-${n}`)) n += 1;
  return `${kind}-${n}`;
}

/**
 * The graph with a new step of `kind`, depending on `from`. Unchanged when
 * `from` is not in it: a dependency on a step that does not exist is a flow the
 * engine refuses to run, and the gesture would look like it had worked.
 */
export function withStepWiredTo(
  graph: Graph,
  kind: StepKind,
  from: string,
): { graph: Graph; id: string | null } {
  const action = DEFAULT_ACTION_FOR_KIND[kind];
  if (!action) return { graph, id: null };
  if (!graph.steps.some((step) => step.id === from)) return { graph, id: null };
  const id = freeStepId(graph, kind);
  const step: Step = {
    id,
    deps: [from],
    input_schema: { type: "any" },
    output_schema: { type: "any" },
    with: null,
    when: null,
    action,
    max_attempts: 1,
  };
  return { graph: { ...graph, steps: [...graph.steps, step] }, id };
}
