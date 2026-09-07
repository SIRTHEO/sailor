// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { StepPassage, Why } from "./engine";
import { StepHistory } from "./StepHistory";

/**
 * **THE REASON IS NOT BEHIND THE CLICK.** An answer only whoever already
 * suspects goes looking for is an answer to nobody.
 */

afterEach(cleanup);

const SKIPPED: Why = {
  because: "condition",
  held: false,
  looked_at: "/report/status",
  wanted: 'equals "failed"',
  found: "passed",
  found_was_there: true,
};

function pretendShell(passages: StepPassage[]) {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(passages) },
  };
  return () => {
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
  };
}

function passage(why: Why | null): StepPassage {
  return {
    run_id: "r-old",
    attempt: 1,
    started_at: 100,
    ended_at: 101,
    outcome: "Skipped",
    failure_class: null,
    refusal: null,
    ran: null,
    why,
    started_by: "window",
    input: {},
    mandate: null,
    signal_who: null,
    signal_where: null,
    said: null,
    output: null,
  };
}

describe("why a step did what it did, in its history", () => {
  test("READS IN THE COLLAPSED ROW, without opening the passage", async () => {
    const stop = pretendShell([passage(SKIPPED)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      const line = await screen.findByText(/report\/status/);
      expect(line.className, "the reason is drawn by the reason's own element").toContain(
        "step-why",
      );
      expect(
        container.querySelector(".passage__detail"),
        "nothing was clicked, so the detail must still be shut",
      ).toBeNull();
      expect(line.textContent).toContain('"passed"');
    } finally {
      stop();
    }
  });

  test("and a step the engine never judged shows no reason at all", async () => {
    const stop = pretendShell([passage(null)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      await screen.findByRole("button", { expanded: false });
      expect(
        container.querySelector(".step-why"),
        "a step with no condition has no reason, and an empty line reads as a lost datum",
      ).toBeNull();
    } finally {
      stop();
    }
  });
});
