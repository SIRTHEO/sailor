import { describe, expect, test } from "vitest";
import type { Graph, Step, StepKind } from "./flow";
import { freeStepId, withStepWiredTo } from "./wiring";

function step(id: string, deps: string[] = []): Step {
  return {
    id,
    deps,
    input_schema: { type: "any" },
    output_schema: { type: "any" },
    with: null,
    when: null,
    action: "shell_check",
    max_attempts: 1,
  };
}

const graph: Graph = { steps: [step("read"), step("check-1", ["read"])] };

describe("a step born wired to the one it came from", () => {
  test("it arrives depending on its source, in one edit", () => {
    const { graph: next, id } = withStepWiredTo(graph, "check", "read");
    expect(id).toBe("check-2");
    const born = next.steps.find((s) => s.id === id);
    expect(born?.deps).toEqual(["read"]);
    expect(next.steps).toHaveLength(3);
  });

  /**
   * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. A dependency on a step
   * that is not in the graph is a flow the engine refuses to run, and the
   * gesture would look like it had worked.
   */
  test("a source that is not in the graph writes nothing", () => {
    const { graph: next, id } = withStepWiredTo(graph, "check", "ghost");
    expect(id).toBeNull();
    expect(next).toBe(graph);
  });

  test("a family with no action writes nothing, rather than an unsaveable node", () => {
    const { id } = withStepWiredTo(graph, "wait" as StepKind, "read");
    expect(id).toBeNull();
  });

  test("the source keeps its own dependencies", () => {
    const { graph: next } = withStepWiredTo(graph, "check", "check-1");
    expect(next.steps.find((s) => s.id === "check-1")?.deps).toEqual(["read"]);
  });
});

describe("the name a new step gets", () => {
  test("it is the first of its family nobody is using", () => {
    expect(freeStepId(graph, "check")).toBe("check-2");
    expect(freeStepId(graph, "engine")).toBe("engine-1");
  });

  /* Counting the family's members would reuse a name after a deletion, and two
     steps with one id is a graph the engine cannot resolve. */
  test("a gap in the numbering is filled, not skipped past", () => {
    const gappy: Graph = { steps: [step("check-1"), step("check-3")] };
    expect(freeStepId(gappy, "check")).toBe("check-2");
  });
});
