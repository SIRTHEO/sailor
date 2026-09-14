// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { RunningNow, runningNow, costWords } from "./RunningNow";
import type { RunSnapshot } from "./engine";

afterEach(cleanup);

function run(over: Partial<RunSnapshot>): RunSnapshot {
  return { run_id: "r", flow: "take-the-next-work", started_at: 0, status: "running", events: [], ...over };
}

describe("runningNow", () => {
  test("keeps only runs still running", () => {
    const runs = [run({ run_id: "a", status: "running" }), run({ run_id: "b", status: "went" })];
    expect(runningNow(runs).map((r) => r.run_id)).toEqual(["a"]);
  });

  test("newest first", () => {
    const runs = [run({ run_id: "old", started_at: 100 }), run({ run_id: "new", started_at: 200 })];
    expect(runningNow(runs).map((r) => r.run_id)).toEqual(["new", "old"]);
  });

  test("empty when nothing is running", () => {
    expect(runningNow([run({ status: "went" }), run({ status: "broke" })])).toEqual([]);
  });
});

describe("costWords", () => {
  test("unknown cost is said, never shown as zero", () => {
    expect(costWords(null)).toBe("cost unknown");
  });

  test("micros become dollars to two places", () => {
    expect(costWords(1_500_000)).toBe("$1.50");
    expect(costWords(0)).toBe("$0.00");
  });
});

describe("RunningNow", () => {
  test("draws nothing when nothing runs", () => {
    const { container } = render(<RunningNow rows={[]} now={1000} />);
    expect(container.textContent).toBe("");
  });

  test("names the flow, how long, and the cost", () => {
    render(<RunningNow rows={[{ run: run({ started_at: 940 }), costMicros: 150_000 }]} now={1000} />);
    expect(screen.getByText("take-the-next-work")).toBeTruthy();
    expect(screen.getByText(/1m ago · \$0\.15/)).toBeTruthy();
  });

  test("an unread cost says so, not a silent zero", () => {
    render(<RunningNow rows={[{ run: run({}), costMicros: null }]} now={1000} />);
    expect(screen.getByText(/cost unknown/)).toBeTruthy();
  });

  test("clicking a row calls onRun with its id", () => {
    const onRun = vi.fn();
    render(<RunningNow rows={[{ run: run({ run_id: "the-run" }), costMicros: 0 }]} now={1000} onRun={onRun} />);
    fireEvent.click(screen.getByText("take-the-next-work"));
    expect(onRun).toHaveBeenCalledWith("the-run");
  });
});
