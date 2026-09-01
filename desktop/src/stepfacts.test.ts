import { describe, expect, test } from "vitest";
import type { Graph, Step } from "./flow";
import { mandateOf, neighboursOf } from "./stepfacts";

function step(id: string, deps: string[], params: Record<string, unknown> | null = null): Step {
  return {
    id,
    deps,
    input_schema: { type: "any" },
    output_schema: { type: "any" },
    with: params,
    when: null,
    action: "external_engine",
    max_attempts: 1,
  };
}

const graph: Graph = {
  steps: [step("read", []), step("write", ["read"]), step("check", ["write"]), step("report", ["write"])],
};

describe("the mandate a step was given", () => {
  test("the prompt it declares is the mandate", () => {
    expect(mandateOf(step("x", [], { prompt: "repair the node" }), null)).toBe("repair the node");
  });

  /**
   * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. A step can leave its
   * prompt open and receive it at run time: reading only `with` would show an
   * empty mandate for exactly the steps whose mandate someone chose by hand.
   */
  test("what it received wins over what it declared, because that is what ran", () => {
    expect(mandateOf(step("x", [], { prompt: "a placeholder" }), { prompt: "what was really asked" })).toBe(
      "what was really asked",
    );
  });

  test("an engine fed through its stdin has that for a mandate", () => {
    expect(mandateOf(step("x", [], null), { stdin: "do this thing" })).toBe("do this thing");
  });

  test("a step with no mandate says so, instead of showing an empty box", () => {
    expect(mandateOf(step("x", [], { command: "cargo test" }), { command: "cargo test" })).toBe(null);
  });
});

describe("the step before and the step after", () => {
  test("both sides are read from the graph", () => {
    expect(neighboursOf(graph, "write")).toEqual({ before: ["read"], after: ["check", "report"] });
  });

  test("the first step has nothing before it, which is not the same as being unknown", () => {
    expect(neighboursOf(graph, "read")).toEqual({ before: [], after: ["write"] });
  });

  test("a step that is not in the graph has no neighbours either way", () => {
    expect(neighboursOf(graph, "absent")).toEqual({ before: [], after: [] });
  });
});
