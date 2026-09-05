// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { Refusal, RunEvent, RunSnapshot, StepPassage } from "./engine";
import { RunConsole, panesFromEvents, readRefusal } from "./RunConsole";
import { StepHistory } from "./StepHistory";

/**
 * **A REFUSAL IS SHOWN AS STRUCTURE, NOT ONLY AS PROSE.** The ledger keeps
 * which check refused, by which rule, at which path and what it saw; until
 * this the window showed a failed step through its sentence alone, and a
 * person could not see the rule or the path without reading the whole text.
 */

afterEach(cleanup);

const OFF_SHAPE: Refusal = { check: "answer_shape", path: "$.verdict", rule: "not_allowed", seen: '"remvoe"' };

function started(seq: number, stepId: string): RunEvent {
  return { run_id: "r", seq, kind: "step_started", at: seq, step_id: stepId, payload: { attempt: 1, input: {} } };
}

function broke(seq: number, stepId: string, refusal: Refusal | null): RunEvent {
  return {
    run_id: "r",
    seq,
    kind: "step_closed",
    at: seq,
    step_id: stepId,
    payload: { outcome: "Broke", failure_class: "answer_off_shape", said: "off shape", refusal },
  };
}

function runOf(events: RunEvent[]): RunSnapshot {
  return { run_id: "r", flow: "prova", started_at: 0, status: "broke", events };
}

function consoleOf(events: RunEvent[]) {
  const run = runOf(events);
  return render(
    <RunConsole
      run={run}
      runs={[run]}
      mode="split"
      now={10}
      listenFailure={null}
      usage={null}
      onMode={() => undefined}
      onPick={() => undefined}
      onClose={() => undefined}
      onStop={() => Promise.resolve()}
    />,
  );
}

describe("a failed step in the run view", () => {
  test("WITH A REFUSAL, THE RULE, THE PATH AND THE EXCERPT EACH HAVE THEIR OWN ELEMENT", () => {
    const { container } = consoleOf([started(1, "verdict"), broke(2, "verdict", OFF_SHAPE)]);

    const refusal = container.querySelector(".step-refusal");
    expect(refusal?.getAttribute("data-rule")).toBe("not_allowed");
    expect(refusal?.getAttribute("data-check")).toBe("answer_shape");
    expect(container.querySelector(".step-refusal__check")?.textContent).toBe("refused by answer_shape");
    expect(container.querySelector(".step-refusal__rule")?.textContent).toBe(
      "the value is not among the allowed ones",
    );
    expect(container.querySelector(".step-refusal__path")?.textContent).toBe("at $.verdict");
    expect(container.querySelector(".step-refusal__seen")?.textContent).toBe('"remvoe"');

    // The prose stays, below the structure: the class in a sentence.
    const failure = container.querySelector(".pane__failure");
    expect(failure?.textContent).toBe("the answer is not in the declared shape");
    expect(refusal?.compareDocumentPosition(failure as Node) ?? 0).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  test("a refusal of the whole value says so instead of showing an empty path", () => {
    const { container } = consoleOf([
      started(1, "verdict"),
      broke(2, "verdict", { check: "answer_shape", path: "", rule: "not_json", seen: "non sono json" }),
    ]);
    expect(container.querySelector(".step-refusal__path")?.textContent).toBe("the whole value");
    expect(container.querySelector(".step-refusal__rule")?.textContent).toBe("the answer is not JSON");
  });

  test("a rule the window has never heard of shows its name, not an invented sentence", () => {
    const { container } = consoleOf([
      started(1, "verdict"),
      broke(2, "verdict", { ...OFF_SHAPE, rule: "a_rule_from_a_newer_engine" }),
    ]);
    expect(container.querySelector(".step-refusal__rule")?.textContent).toBe("a_rule_from_a_newer_engine");
  });

  test("WITHOUT A REFUSAL, ONLY THE MESSAGE IS SHOWN", () => {
    const { container } = consoleOf([started(1, "verdict"), broke(2, "verdict", null)]);
    expect(container.querySelector(".step-refusal")).toBeNull();
    expect(container.querySelector(".pane__failure")?.textContent).toBe("the answer is not in the declared shape");
  });

  test("the pane carries the refusal whole, and a half one is no refusal", () => {
    const panes = panesFromEvents([started(1, "verdict"), broke(2, "verdict", OFF_SHAPE)]);
    expect(panes[0]?.refusal).toEqual(OFF_SHAPE);
    expect(readRefusal({ check: "answer_shape" })).toBeNull();
    expect(readRefusal("refused")).toBeNull();
    expect(readRefusal(null)).toBeNull();
  });
});

function pretendShell(passages: StepPassage[]) {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(passages) },
  };
  return () => {
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
  };
}

function passageWith(refusal: Refusal | null): StepPassage {
  return {
    run_id: "r-old",
    attempt: 1,
    started_at: 100,
    ended_at: 101,
    outcome: "Broke",
    failure_class: "answer_off_shape",
    refusal,
    ran: null,
    started_by: "window",
    input: {},
    mandate: null,
    signal_who: null,
    signal_where: null,
    said: "off shape",
    output: null,
  };
}

describe("a failed step in its history", () => {
  test("AN OLD REFUSAL READS THE SAME WAY, from the ledger", async () => {
    const stop = pretendShell([passageWith(OFF_SHAPE)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      fireEvent.click(await screen.findByRole("button", { expanded: false }));
      expect(container.querySelector(".step-refusal__rule")?.textContent).toBe(
        "the value is not among the allowed ones",
      );
      expect(container.querySelector(".step-refusal__path")?.textContent).toBe("at $.verdict");
      expect(container.querySelector(".step-refusal__seen")?.textContent).toBe('"remvoe"');
      expect(container.querySelector(".passage__failure")).not.toBeNull();
    } finally {
      stop();
    }
  });

  test("a passage without a refusal shows only the message", async () => {
    const stop = pretendShell([passageWith(null)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      fireEvent.click(await screen.findByRole("button", { expanded: false }));
      expect(container.querySelector(".step-refusal")).toBeNull();
      expect(container.querySelector(".passage__failure")).not.toBeNull();
    } finally {
      stop();
    }
  });
});
