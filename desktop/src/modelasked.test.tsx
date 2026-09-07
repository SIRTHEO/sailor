// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Calls, modelReading } from "./History";
import type { ModelCall } from "./engine";

/**
 * **A RUN PRICED AT THE WRONG MODEL READ AS A RUN THAT HAD CHOSEN IT**: the
 * model asked for crossed the wire undrawn, and so did the turns.
 */

const call = (over: Partial<ModelCall> = {}): ModelCall => ({
  call_id: "c1",
  step_id: "review",
  purpose: "writing",
  cli: "an-engine",
  requested_model: "",
  actual_model: "a-model",
  input_tokens: 10,
  output_tokens: 5,
  cached_tokens: 0,
  cache_write_tokens: 0,
  total_tokens: 15,
  turns: 3,
  cost_micros: 1000,
  declared_cost_micros: null,
  error_type: null,
  started_at: 1,
  ended_at: 2,
  ...over,
});

describe("the model a call actually ran on", () => {
  test("SAYS THE ONE ASKED FOR WHEN ANOTHER ANSWERED", () => {
    expect(modelReading(call({ requested_model: "a-strong-one", actual_model: "a-cheap-one" }))).toBe(
      "a-cheap-one — asked for a-strong-one",
    );
  });

  test("and asking for nothing is not a substitution", () => {
    expect(
      modelReading(call({ requested_model: "", actual_model: "a-model" })),
      "the flow named no model, so the engine's default is the declared fallback",
    ).toBe("a-model");
    expect(modelReading(call({ requested_model: "a-model", actual_model: "a-model" }))).toBe("a-model");
  });

  test("a model nobody declared still says which was asked for", () => {
    const said = modelReading(call({ requested_model: "a-strong-one", actual_model: "" }));
    expect(said, "the answer is unknown; the request is not").toContain("a-strong-one");
  });

  test("THE TURNS ARE ON SCREEN: a token total says nothing about where it went", () => {
    render(<Calls calls={[call({ turns: 42 })]} />);
    expect(screen.getByText("42"), "the turns crossed the wire and nothing drew them").toBeTruthy();
  });

  test("and a call that declared no turns says so rather than counting zero", () => {
    render(<Calls calls={[call({ turns: null })]} />);
    expect(screen.queryByText("0"), "«not said» must never be drawn as none").toBeNull();
  });
});
