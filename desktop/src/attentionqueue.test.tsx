// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { AttentionQueue } from "./AttentionQueue";
import { rankAttentionRows, type AttentionRow } from "./attention";

afterEach(cleanup);

const NOW = 1_000_000;

describe("AttentionQueue", () => {
  test("ranking orders handed -> cap_reached -> engine_unreachable -> terminal_dead, ties by since oldest first", () => {
    const rows: AttentionRow[] = [
      {
        kind: "terminal_dead",
        tty: "ttys003",
        status_word: "stopped",
        reason: "abandoned terminal",
        since: null,
        link: { kind: "tty", tty: "ttys003" },
      },
      {
        kind: "engine_unreachable",
        status_word: "unreachable",
        reason: "quota offline",
        since: null,
      },
      {
        kind: "cap_reached",
        run_id: "r-cap",
        status_word: "at its cap",
        reason: "cost ceiling reached",
        since: NOW - 100,
        link: { kind: "run", run_id: "r-cap" },
      },
      {
        kind: "handed",
        run_id: "r-handed-2",
        step_id: "s2",
        status_word: "waiting on you",
        reason: "second decision",
        since: NOW - 500,
        link: { kind: "run", run_id: "r-handed-2" },
      },
      {
        kind: "handed",
        run_id: "r-handed-1",
        step_id: "s1",
        status_word: "waiting on you",
        reason: "first decision",
        since: NOW - 1000,
        link: { kind: "run", run_id: "r-handed-1" },
      },
    ];

    const ranked = rankAttentionRows(rows);
    expect(ranked.map((r) => r.reason)).toEqual([
      "first decision",
      "second decision",
      "cost ceiling reached",
      "quota offline",
      "abandoned terminal",
    ]);

    render(<AttentionQueue rows={ranked} now={NOW} />);
    const renderedReasons = Array.from(document.querySelectorAll(".waiting__what")).map(
      (el) => el.textContent,
    );
    expect(renderedReasons).toEqual([
      "first decision",
      "second decision",
      "cost ceiling reached",
      "quota offline",
      "abandoned terminal",
    ]);
  });

  test("reason unknown fallback when reason is empty or whitespace", () => {
    const rows: AttentionRow[] = [
      {
        kind: "handed",
        run_id: "r-unknown",
        step_id: "s-unknown",
        status_word: "waiting on you",
        reason: "   ",
        since: NOW - 60,
      },
    ];

    render(<AttentionQueue rows={rows} now={NOW} />);
    expect(screen.getByText("reason unknown")).toBeTruthy();
  });

  test("{kind: 'run'} routes to run when two terminals share a worktree, and {kind: 'tty'} routes to tty", () => {
    const runCalls: string[] = [];
    const ttyCalls: string[] = [];

    const rows: AttentionRow[] = [
      {
        kind: "handed",
        run_id: "run-ambiguous",
        step_id: "s1",
        status_word: "waiting on you",
        reason: "needs approval",
        since: NOW - 100,
        link: { kind: "run", run_id: "run-ambiguous" },
      },
      {
        kind: "terminal_dead",
        tty: "ttys001",
        status_word: "stopped",
        reason: "dead shell",
        since: null,
        link: { kind: "tty", tty: "ttys001" },
      },
    ];

    render(
      <AttentionQueue
        rows={rows}
        now={NOW}
        onRun={(runId) => runCalls.push(runId)}
        onTty={(tty) => ttyCalls.push(tty)}
      />,
    );

    const openRunBtn = screen.getByRole("button", { name: "Open the run" });
    openRunBtn.click();
    expect(runCalls).toEqual(["run-ambiguous"]);

    const openTtyBtn = screen.getByRole("button", { name: "Open terminal ttys001" });
    openTtyBtn.click();
    expect(ttyCalls).toEqual(["ttys001"]);
  });

  test("none of the prohibited elements are rendered: no rings, no %, no health score, no spinner, and no waiting on you for rows that offer no action", () => {
    const rows: AttentionRow[] = [
      {
        kind: "handed",
        run_id: "r-actionable",
        step_id: "s-act",
        status_word: "waiting on you",
        reason: "actionable step",
        since: NOW - 100,
        link: { kind: "run", run_id: "r-actionable" },
      },
      {
        kind: "cap_reached",
        run_id: "r-cap",
        status_word: "waiting on you",
        reason: "stopped at cap",
        since: NOW - 200,
      },
      {
        kind: "engine_unreachable",
        status_word: "unreachable",
        reason: "auth failed",
        since: null,
      },
      {
        kind: "terminal_dead",
        tty: "ttys002",
        status_word: "stopped",
        reason: "abandoned",
        since: null,
        link: { kind: "tty", tty: "ttys002" },
      },
    ];

    render(<AttentionQueue rows={rows} now={NOW} onRun={() => {}} onTty={() => {}} />);

    expect(document.querySelector("circle")).toBeNull();
    expect(document.querySelector("meter")).toBeNull();
    expect(document.querySelector('[role="progressbar"]')).toBeNull();
    expect(document.body.textContent).not.toContain("%");
    expect(document.body.textContent?.toLowerCase()).not.toContain("health");
    expect(document.querySelector(".animate-spin")).toBeNull();
    expect(document.querySelector("[data-spinner]")).toBeNull();

    const capRow = document.querySelector('[data-kind="cap_reached"]');
    expect(capRow?.textContent).not.toContain("waiting on you");

    const engineRow = document.querySelector('[data-kind="engine_unreachable"]');
    expect(engineRow?.textContent).not.toContain("waiting on you");

    const deadRow = document.querySelector('[data-kind="terminal_dead"]');
    expect(deadRow?.textContent).not.toContain("waiting on you");

    const handedRow = document.querySelector('[data-kind="handed"]');
    expect(handedRow?.textContent).toContain("waiting on you");
  });
});
