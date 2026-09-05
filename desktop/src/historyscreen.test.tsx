// @vitest-environment jsdom
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import catalogue from "../../i18n/en.json";
import type { Execution, ModelCall } from "./engine";
import { History } from "./History";

// A call that declared no model is one sentence, and the command line already
// has it. Two copies of a sentence drift apart in silence, and only one of
// them can be translated: the window says it through the catalogue key.

afterEach(cleanup);

function callOf(actualModel: string): ModelCall {
  return {
    call_id: "c1",
    step_id: "verdict",
    purpose: "answer",
    cli: "engine",
    requested_model: "",
    actual_model: actualModel,
    input_tokens: 10,
    output_tokens: 5,
    cached_tokens: null,
    cache_write_tokens: null,
    total_tokens: 15,
    turns: 1,
    cost_micros: 0,
    declared_cost_micros: null,
    error_type: null,
    started_at: 100,
    ended_at: 101,
  };
}

function runWith(call: ModelCall): Execution {
  return {
    run_id: "r1",
    kind: "flow",
    entity: "prova",
    worktree: null,
    status: "succeeded",
    started_at: 100,
    ended_at: 101,
    duration_secs: 1,
    total_cost_micros: 0,
    error: null,
    steps_total: 1,
    steps_went: 1,
    steps_broke: 0,
    steps_retried: 0,
    steps_open: [],
    tokens: {
      input_tokens: 10,
      output_tokens: 5,
      cached_tokens: 0,
      cache_write_tokens: 0,
      cost_micros: 0,
      calls: 1,
      calls_without_tokens: 0,
      calls_without_cost: 1,
    },
    tokens_by_model: {},
    calls: [call],
  };
}

function pretendShell(runs: Execution[]) {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(runs) },
  };
  return () => {
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
  };
}

async function modelCellOf(call: ModelCall): Promise<string> {
  const stop = pretendShell([runWith(call)]);
  try {
    const { container } = render(<History native root={null} />);
    await waitFor(() => expect(container.querySelector(".calls")).not.toBeNull());
    return container.querySelectorAll(".calls .now__when")[2]?.textContent ?? "";
  } finally {
    stop();
  }
}

describe("a call that declared no model", () => {
  test("IS SAID WITH THE SENTENCE THE CATALOGUE HOLDS, not one typed here", async () => {
    const said = await modelCellOf(callOf(""));
    expect(said).toBe((catalogue as Record<string, string>)["ui.cost.model_not_declared"]);
  });

  test("a call that declared one shows the model itself", async () => {
    expect(await modelCellOf(callOf("a-model"))).toBe("a-model");
  });
});
