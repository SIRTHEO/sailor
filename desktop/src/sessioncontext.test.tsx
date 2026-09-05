// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Execution } from "./engine";
import type { RunUsage } from "./flow";
import { SessionContext } from "./SessionContext";
import { costReading } from "./RunConsole";
import { recordedRun } from "./recordedWork";
import type { TerminalSummary } from "./terminal";
import stylesheet from "./styles.css?raw";
import { belowThreshold, contrastPairs, inOtherScheme, parseStylesheet } from "./contrast";

afterEach(() => { cleanup(); delete (window as unknown as { __TAURI__?: unknown }).__TAURI__; });

const terminal: TerminalSummary = { id: "one", workspaceRoot: "/work/project", workspaceName: "project", program: "engine", profile: null, alive: true, processId: 1, device: "ttys001", moved: 0, estimatedTokens: 0 };
const tokens: RunUsage["tokens"] = { calls: 2, calls_without_cost: 1, calls_without_tokens: 0, cost_micros: 1000000, input_tokens: 10, output_tokens: 20, cached_tokens: 0, cache_write_tokens: 0, total_tokens_only: 0 };
const run: Execution = { run_id: "run-one", kind: "flow", entity: "build", worktree: terminal.workspaceRoot, status: "succeeded", started_at: 1, ended_at: 4, duration_secs: 3, total_cost_micros: 1000000, error: null, steps_total: 1, steps_went: 1, steps_broke: 0, steps_retried: 0, steps_open: [], tokens, tokens_by_model: {}, calls: [] };
const usage: RunUsage = { ...run, tokens, tokens_by_model: {}, calls: [] };
const refusal = { check: "gate", rule: "not_allowed", path: "src", seen: "missing authority" };
const rows = [["check", 1, 2, 3, "{}", null, "Broke", "refused the write", "gate_refused", JSON.stringify(refusal), null]];

function bridge(overrides: Record<string, unknown> = {}) {
  const answers: Record<string, unknown> = {
    execution_history: [run, { ...run, run_id: "unrelated", worktree: "/work/elsewhere", started_at: 99 }],
    ledger_query: { columns: [], rows, truncated: false }, run_usage: usage,
    quota: [{ engine: "engine", unit: "five_hour", spent_fraction: 0.25, observed_at: 1, resets_at: null }],
    models_catalogue: { models: [], choices: [] }, ...overrides,
  };
  const invoke = vi.fn(async (command: string, _args?: unknown) => {
    const answer = answers[command];
    if (answer instanceof Error) throw answer;
    return answer;
  });
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = { core: { invoke } };
  return invoke;
}

test("the session shows folder-scoped facts and opens every existing detail alongside it", async () => {
  const invoke = bridge();
  const { container } = render(<div className="session-work"><div data-testid="terminal" /><SessionContext native terminal={terminal} lines={[]} /></div>);
  const pane = screen.getByTestId("terminal");
  await screen.findByText(/75.0% remaining/);
  await screen.findByText(/check \(attempt 1\) · Broke/);
  expect(screen.queryByRole("option", { name: /unrelated/ })).toBeNull();
  expect(screen.getByRole("button", { name: /Current run cost/ }).textContent).toContain("at least $1.0000");
  expect(screen.getByRole("button", { name: /Current run cost/ }).textContent).toContain("1 calls out of 2 are not measured");
  fireEvent.click(screen.getByRole("button", { name: /Refusals/ }));
  expect(container.querySelector(".step-refusal")?.textContent).toContain("missing authority");
  fireEvent.click(screen.getByRole("button", { name: /Last step outcome/ }));
  expect(container.querySelector(".console")?.textContent).toContain("refused the write");
  fireEvent.click(screen.getByRole("button", { name: /Current run cost/ }));
  expect(container.querySelector(".session-context__detail")?.textContent).toContain("not measured");
  fireEvent.click(screen.getByRole("button", { name: /Engine quota/ }));
  await screen.findByText("Quota and models");
  expect(invoke.mock.calls.filter(([command]) => command === "quota")).toHaveLength(1);
  expect(screen.getByTestId("terminal")).toBe(pane);
  expect(pane.closest("[hidden]")).toBeNull();
});

test("unreadable facts never become an empty success or a zero quota", async () => {
  bridge({ ledger_query: new Error("ledger locked"), run_usage: new Error("cost unavailable"), quota: new Error("quota refused") });
  render(<SessionContext native terminal={terminal} lines={[]} />);
  await screen.findByText(/quota refused/);
  await waitFor(() => expect(screen.getByRole("button", { name: /Last step outcome/ }).textContent).toContain("ledger locked"));
  expect(screen.getByRole("button", { name: /Refusals/ }).textContent).toContain("ledger locked");
  expect(screen.getByRole("button", { name: /Current run cost/ }).textContent).toContain("cost unavailable");
  expect(screen.queryByText(/0 recorded refusals/)).toBeNull();
});

test("a session change drops the old folder's run and does not borrow another engine's quota", async () => {
  bridge();
  const { rerender } = render(<SessionContext native terminal={terminal} lines={[]} />);
  await screen.findByText(/75.0% remaining/);
  rerender(<SessionContext native terminal={{ ...terminal, id: "two", workspaceRoot: "/work/empty", program: "other-engine" }} lines={[]} />);
  expect(screen.getByText("No recorded run in this folder")).toBeTruthy();
  expect(screen.getByText(/not reported for other-engine/)).toBeTruthy();
  expect(screen.queryByText(/at least/)).toBeNull();
});

test("every cost reading puts gaps in the number's place", () => {
  expect(costReading(usage)).toContain("at least");
  expect(costReading({ ...usage, tokens: { ...tokens, calls_without_cost: 2 } })).toContain("unknown");
  expect(costReading({ ...usage, tokens: { ...tokens, calls_without_cost: 0 } })).not.toContain("at least");
  expect(costReading({ ...usage, tokens: { ...tokens, calls: 0, calls_without_cost: 0 } })).toContain("no call");
});

test("the recorded reader quotes run ids and preserves attempts, refusals and truncation", async () => {
  const invoke = bridge({ ledger_query: { columns: [], rows: [...rows, ["check", 2, 4, 5, "{}", "{}", "Went", null, null, null, null]], truncated: true } });
  const answer = await recordedRun({ ...run, run_id: "one'two" });
  expect(JSON.stringify(invoke.mock.calls)).toContain("one''two");
  expect(answer.truncated).toBe(true);
  expect(new Set(answer.run.events.map((event) => event.step_id)).size).toBe(2);
  expect(answer.run.events.filter((event) => event.kind === "step_closed")).toHaveLength(2);
  expect(answer.run.events.find((event) => (event.payload as { refusal?: unknown }).refusal)?.payload).toMatchObject({ refusal });
});

test("the attention band has measured contrast in both themes", async () => {
  bridge();
  render(<SessionContext native terminal={terminal} lines={[]} />);
  await screen.findByText(/75.0% remaining/);
  await screen.findByText(/check \(attempt 1\) · Broke/);
  const sheet = parseStylesheet(stylesheet);
  for (const theme of [sheet, inOtherScheme(sheet)]) {
    const pairs = contrastPairs(document.documentElement, theme);
    expect(pairs.length).toBeGreaterThan(8);
    expect(belowThreshold(pairs)).toEqual([]);
  }
});
