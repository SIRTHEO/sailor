// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Spend, byModel } from "./RunConsole";
import type { RunUsage, TokenTotals } from "./flow";

/**
 * **A RUN SAID WHAT IT COST AND NEVER WHO HAD COST IT.** The split by engine
 * has been on the wire since the dashboard summed it; no screen read it.
 */

const totals = (over: Partial<TokenTotals> = {}): TokenTotals => ({
  input_tokens: 1000,
  output_tokens: 200,
  cached_tokens: 0,
  cache_write_tokens: 0,
  total_tokens_only: 0,
  cost_micros: 0,
  calls: 1,
  turns: 1,
  calls_without_tokens: 0,
  calls_without_cost: 0,
  ...over,
});

const usage = (by: Record<string, TokenTotals>): RunUsage => ({
  run_id: "r",
  entity: "build",
  status: "succeeded",
  total_cost_micros: 0,
  steps_total: 1,
  steps_went: 1,
  steps_broke: 0,
  tokens: totals({ calls: 3 }),
  tokens_by_model: by,
  calls: [],
});

describe("which engines answered", () => {
  test("THE DEAREST COMES FIRST, and a tie still comes out in one order", () => {
    const rows = byModel(
      usage({
        cheap: totals({ cost_micros: 10_000 }),
        dear: totals({ cost_micros: 900_000 }),
        bravo: totals({ cost_micros: 10_000 }),
      }),
    );
    expect(rows.map((row) => row.model)).toEqual(["dear", "bravo", "cheap"]);
  });

  test("a price nobody knows is not read as free", () => {
    const rows = byModel(
      usage({
        a: totals({ calls: 2, calls_without_cost: 2, cost_micros: 0 }),
        b: totals({ calls: 2, calls_without_cost: 1, cost_micros: 500_000 }),
      }),
    );
    const none = rows.find((row) => row.model === "a");
    const some = rows.find((row) => row.model === "b");
    expect(none?.price, "no price at all must never print as $0").toBe("price unknown");
    expect(some?.price, "a total missing one call's price is a floor, not the figure").toContain(
      "at least",
    );
  });

  test("THE LINE NAMES THE ENGINE AND HOW MANY TIMES IT ANSWERED", () => {
    render(<Spend usage={usage({ "gpt-6-astra": totals({ calls: 4, cost_micros: 310_000 }) })} />);
    const line = screen.getByText(/gpt-6-astra/);
    expect(line.textContent).toContain("×4");
    expect(line.textContent).toContain("$0.31");
  });

  test("a run no engine answered draws no engines", () => {
    expect(byModel(usage({}))).toEqual([]);
  });
});
