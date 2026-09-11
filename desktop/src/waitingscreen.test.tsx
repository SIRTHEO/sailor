// @vitest-environment jsdom
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { Execution, HandedStep, OpenRun } from "./engine";
import type { Window as QuotaWindow } from "./quota";
import { WaitingScreen } from "./WaitingScreen";
import { inOrder, outcomeOfRun, type Sources } from "./waiting";

/**
 * **THE MORNING SCREEN IS JUDGED ON WHAT IT REFUSES TO SAY.** An empty list and
 * a source that did not answer draw the same blank page unless somebody makes
 * them different, a state carried by tint alone reaches nobody who cannot see
 * it, and an order nobody asserted is the order the sources happened to arrive.
 */

afterEach(cleanup);

const NOW = 1_000_000;
const AWAY = NOW - 12 * 3600;

function answered(sources: Partial<Sources>): Sources {
  return {
    open: { state: "answered", value: [] },
    handed: { state: "answered", value: {} },
    history: { state: "answered", value: [] },
    quota: { state: "answered", value: { windows: [], unreachable: [] } },
    terminals: { state: "answered", value: { answer: "seen", ttys: [] } },
    ...sources,
  };
}

function openRun(runId: string, state: "working" | "waiting"): OpenRun {
  return {
    run_id: runId,
    entity: `flow-${runId}`,
    state,
    open_steps: 1,
    open_now: [],
    since: NOW - 3600,
    started_here: false,
    steps_done: 1,
    steps_total: 3,
  };
}

function handedStep(stepId: string, since: number): HandedStep {
  return {
    step_id: stepId,
    holder: "someone",
    mandate: `answer about ${stepId}\nand a second line nobody puts in a row`,
    since,
    worktree: null,
  };
}

function execution(runId: string, status: string, endedAt: number, over: Partial<Execution> = {}): Execution {
  return {
    run_id: runId,
    kind: "flow",
    entity: `flow-${runId}`,
    worktree: null,
    status,
    started_at: endedAt - 60,
    ended_at: endedAt,
    duration_secs: 60,
    total_cost_micros: 0,
    error: null,
    steps_total: 3,
    steps_went: 3,
    steps_broke: 0,
    steps_retried: 0,
    steps_open: [],
    tokens: {
      input_tokens: 0,
      output_tokens: 0,
      cached_tokens: 0,
      cache_write_tokens: 0,
      cost_micros: 0,
      calls: 0,
      turns: 0, total_tokens_only: 0, calls_without_tokens: 0,
      calls_without_cost: 0,
    },
    tokens_by_model: {},
    calls: [],
    ...over,
  };
}

function quotaWindow(engine: string, spentFraction: number): QuotaWindow {
  return {
    engine,
    unit: "five_hour",
    spent_fraction: spentFraction,
    resets_at: null,
    observed_at: NOW - 120,
  };
}

/** What each row says it is, top to bottom. */
function rows(): string[] {
  return Array.from(document.querySelectorAll(".waiting__what")).map((one) => one.textContent ?? "");
}

function shapeOf(state: string): string | null {
  const path = document.querySelector(`.waiting__mark[data-state="${state}"] path`);
  return path?.getAttribute("d") ?? null;
}

describe("nothing waiting, and not being able to tell", () => {
  test("AN EMPTY MORNING SAYS SO PLAINLY", () => {
    render(<WaitingScreen native now={NOW} since={AWAY} sources={answered({})} />);
    expect(screen.getByText("Nothing is waiting for you")).toBeTruthy();
    expect(screen.getByText("and nothing happened while you were away")).toBeTruthy();
    expect(document.querySelector(".waiting__blind")).toBeNull();
    expect(document.body.textContent).not.toContain("I cannot tell you");
  });

  test("A SCREEN THAT COULD NOT ASK READS NOTHING LIKE AN EMPTY ONE", () => {
    render(<WaitingScreen native={false} now={NOW} since={AWAY} />);
    expect(screen.getByText(/I cannot tell you what waits/)).toBeTruthy();
    // The reassuring direction is the wrong one: silence is never «nothing».
    expect(document.body.textContent).not.toContain("Nothing is waiting for you");
  });

  test("EVERY SOURCE MUTE IS SAID WITH THE REASONS, not as an empty list", () => {
    const dead = { state: "mute", why: "the ledger is not there" } as const;
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={{ open: dead, handed: dead, history: dead, quota: dead, terminals: dead }}
      />,
    );
    expect(screen.getByText(/Nothing here answered/)).toBeTruthy();
    expect(document.body.textContent).toContain("the ledger is not there");
    expect(document.body.textContent).not.toContain("Nothing is waiting for you");
  });

  test("ONE SOURCE MUTE MAKES THE COUNT A FLOOR, and names what it is short of", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          open: { state: "answered", value: [openRun("r1", "waiting")] },
          handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
          quota: { state: "mute", why: "the provider refused the reading" },
        })}
      />,
    );
    expect(screen.getByText("At least 1 thing waits for you")).toBeTruthy();
    expect(screen.getByText(/the quota \(the provider refused the reading\)/)).toBeTruthy();
    // The absurd control: with every source answering, the same one row is a
    // count and not a floor.
    cleanup();
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          open: { state: "answered", value: [openRun("r1", "waiting")] },
          handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
        })}
      />,
    );
    expect(screen.getByText("1 thing waits for you")).toBeTruthy();
  });

  test("AN EMPTY LIST WITH A SOURCE MUTE IS NOT AN EMPTY MORNING", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({ quota: { state: "mute", why: "the provider refused the reading" } })}
      />,
    );
    expect(screen.getByText("Nothing I could read is waiting for you")).toBeTruthy();
    expect(document.body.textContent).not.toContain("Nothing is waiting for you");
  });
});

describe("a state is a shape, never a tint alone", () => {
  test("TWO STATES ARE TWO DRAWINGS, and each carries its word", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          open: { state: "answered", value: [openRun("r1", "waiting")] },
          handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
          history: {
            state: "answered",
            value: [execution("r2", "failed", NOW - 3600, { steps_broke: 1, steps_went: 0 })],
          },
        })}
      />,
    );
    const human = shapeOf("human");
    const broke = shapeOf("broke");
    expect(human, "the human mark was not drawn at all").toBeTruthy();
    expect(broke, "the broke mark was not drawn at all").toBeTruthy();
    expect(human).not.toEqual(broke);
    expect(screen.getByText("waiting on you")).toBeTruthy();
    expect(screen.getByText("broke")).toBeTruthy();
  });
});

describe("the order is the cost to the person", () => {
  test("A DECISION THAT FREEZES A RUN OUTRANKS ONE THAT FREEZES NOTHING", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          // Handed last on purpose: an order nobody sorted is the order the
          // sources happened to arrive in.
          quota: { state: "answered", value: { windows: [quotaWindow("claude", 0.97)], unreachable: [] } },
          history: {
            state: "answered",
            value: [execution("r2", "failed", NOW - 3600, { steps_broke: 1, steps_went: 0 })],
          },
          open: { state: "answered", value: [openRun("r1", "waiting")] },
          handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
        })}
      />,
    );
    expect(rows()).toEqual([
      "answer about review",
      "flow-r2 broke",
      "97% of the 5 hours window on claude is spent",
    ]);
  });

  test("WITHIN ONE RANK THE LONGEST WAIT COMES FIRST", () => {
    const ordered = inOrder([
      { id: "b", state: "human", question: "b", context: "", since: 500, runId: "b", stepId: "s" },
      { id: "a", state: "human", question: "a", context: "", since: 100, runId: "a", stepId: "s" },
      { id: "c", state: "quota", question: "c", context: "", since: null, runId: null, stepId: null },
    ]);
    expect(ordered.map((one) => one.id)).toEqual(["a", "b", "c"]);
  });

  test("A RUN STILL WORKING IS NOT A DECISION: it waits on nobody here", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          open: { state: "answered", value: [openRun("r1", "working")] },
          handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
        })}
      />,
    );
    expect(screen.getByText("Nothing is waiting for you")).toBeTruthy();
  });
});

describe("what happened while nobody was watching", () => {
  test("A RUN STOPPED BY ITS OWN CEILING IS A REPORT, not a fault", () => {
    const capped = execution("r3", "cap_reached", NOW - 1800, {
      steps_went: 2,
      error: "stopped by the spending cap: 2.00 of a 2.00 cap",
      total_cost_micros: 2_000_000,
    });
    // The cap is read before the error, or a bounded stop files as a break.
    expect(outcomeOfRun(capped)).toBe("capped");
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({ history: { state: "answered", value: [capped] } })}
      />,
    );
    expect(screen.getByText("Nothing is waiting for you")).toBeTruthy();
    expect(screen.getByText("and 1 thing happened while you were away")).toBeTruthy();
    expect(rows()).toEqual(["flow-r3 at its cap"]);
    expect(screen.getByText(/\$2\.00 · 30m ago/)).toBeTruthy();
  });

  test("A STATUS THIS WINDOW DOES NOT KNOW IS NAMED, never filed under a word", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({
          history: { state: "answered", value: [execution("r4", "a_state_from_a_later_version", NOW - 60)] },
        })}
      />,
    );
    expect(screen.getByText(/a_state_from_a_later_version/)).toBeTruthy();
    expect(rows()).toEqual([]);
  });

  test("NOTHING OLDER THAN THE SPAN IS COUNTED as having happened tonight", () => {
    render(
      <WaitingScreen
        native
        now={NOW}
        since={AWAY}
        sources={answered({ history: { state: "answered", value: [execution("r5", "complete", AWAY - 1)] } })}
      />,
    );
    expect(screen.getByText("and nothing happened while you were away")).toBeTruthy();
  });
});

describe("the gestures a row is allowed to draw", () => {
  test("NO HANDLER, NO BUTTON: a gesture that goes nowhere is not offered", () => {
    const sources = answered({
      open: { state: "answered", value: [openRun("r1", "waiting")] },
      handed: { state: "answered", value: { r1: [handedStep("review", NOW - 600)] } },
    });
    render(<WaitingScreen native now={NOW} since={AWAY} sources={sources} />);
    expect(screen.queryByRole("button", { name: "Open the run" })).toBeNull();
    cleanup();
    const opened: string[] = [];
    render(<WaitingScreen native now={NOW} since={AWAY} sources={sources} onRun={(id) => opened.push(id)} />);
    screen.getByRole("button", { name: "Open the run" }).click();
    expect(opened).toEqual(["r1"]);
  });
});

describe("reading the engine for itself", () => {
  test("ONLY THE STOPPED RUNS ARE ASKED WHAT THEY WANT of a person", async () => {
    const asked: string[] = [];
    const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (command: string, args?: Record<string, unknown>) => {
          asked.push(command);
          if (command === "open_runs") {
            return Promise.resolve([openRun("r1", "waiting"), openRun("r2", "working")]);
          }
          if (command === "handed_steps") {
            expect(args).toEqual({ runId: "r1" });
            return Promise.resolve([handedStep("review", NOW - 600)]);
          }
          if (command === "execution_history") return Promise.resolve([]);
          if (command === "quota") return Promise.resolve({ windows: [], unreachable: [] });
          if (command === "terminals_abandoned") return Promise.resolve({ answer: "seen", ttys: [] });
          return Promise.reject(new Error(`no ${command}`));
        },
      },
    };
    try {
      render(<WaitingScreen native now={NOW} since={AWAY} />);
      await screen.findByText("1 thing waits for you");
      expect(asked.filter((one) => one === "handed_steps")).toHaveLength(1);
    } finally {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    }
  });

  test("A SOURCE THAT THROWS DOES NOT SILENCE THE OTHERS", async () => {
    const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (command: string) => {
          if (command === "open_runs") return Promise.resolve([]);
          if (command === "quota") return Promise.reject(new Error("the provider said no"));
          if (command === "execution_history") {
            return Promise.resolve([execution("r6", "complete", NOW - 300)]);
          }
          return Promise.reject(new Error(`no ${command}`));
        },
      },
    };
    try {
      render(<WaitingScreen native now={NOW} since={AWAY} />);
      await waitFor(() => expect(rows()).toEqual(["flow-r6 went"]));
      expect(screen.getByText(/the quota \(Error: the provider said no\)/)).toBeTruthy();
    } finally {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    }
  });
});
