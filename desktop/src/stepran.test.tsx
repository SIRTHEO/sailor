// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { Ran, RunEvent, RunSnapshot, StepPassage } from "./engine";
import { RunConsole, panesFromEvents, readRan } from "./RunConsole";
import { StepHistory } from "./StepHistory";
import { renderRan } from "./StepRan";

/**
 * **THE WINDOW SHOWS WHAT A STEP RAN.** The ledger keeps the program and the
 * arguments a step started; until this the window showed what came in and what
 * came out, and the command between them was a guess.
 */

afterEach(cleanup);

const SHELL_LINE: Ran = { program: "sh", args: ["-c", "echo hi"] };

function started(seq: number, stepId: string): RunEvent {
  return { run_id: "r", seq, kind: "step_started", at: seq, step_id: stepId, payload: { attempt: 1, input: {} } };
}

function closed(seq: number, stepId: string, ran: Ran | null): RunEvent {
  return {
    run_id: "r",
    seq,
    kind: "step_closed",
    at: seq,
    step_id: stepId,
    payload: { outcome: "Went", failure_class: null, said: null, refusal: null, ran },
  };
}

function consoleOf(events: RunEvent[]) {
  const run: RunSnapshot = { run_id: "r", flow: "prova", started_at: 0, status: "complete", events };
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

describe("the line a step started, word by word", () => {
  test("A WORD WITH A SPACE IN IT, OR NOTHING IN IT, IS QUOTED", () => {
    expect(renderRan(SHELL_LINE)).toBe("sh -c «echo hi»");
    expect(renderRan({ program: "sh", args: ["-c", "echo hi", "", "plain"] })).toBe("sh -c «echo hi» «» plain");
    expect(renderRan({ program: "codex", args: [] })).toBe("codex");
    expect(renderRan({ program: "my engine", args: ["--now"] })).toBe("«my engine» --now");
  });

  test("a line read from a closing fact is the line, and a half one is none", () => {
    expect(readRan({ program: "sh", args: ["-c"] })).toEqual({ program: "sh", args: ["-c"] });
    expect(readRan({ program: "sh" })).toBeNull();
    expect(readRan({ program: "sh", args: [1] })).toBeNull();
    expect(readRan("sh -c")).toBeNull();
    expect(readRan(null)).toBeNull();
  });
});

describe("a step in the run view", () => {
  test("WITH A LINE, THE PROGRAM AND ITS ARGUMENTS ARE SHOWN AS ONE", () => {
    const { container } = consoleOf([started(1, "verdict"), closed(2, "verdict", SHELL_LINE)]);
    expect(container.querySelector(".step-ran")?.textContent).toBe("sh -c «echo hi»");
    expect(panesFromEvents([started(1, "verdict"), closed(2, "verdict", SHELL_LINE)])[0]?.ran).toEqual(SHELL_LINE);
  });

  test("WITHOUT A LINE, NO SUCH ELEMENT IS DRAWN", () => {
    const { container } = consoleOf([started(1, "verdict"), closed(2, "verdict", null)]);
    expect(container.querySelector(".step-ran")).toBeNull();
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

function passageWith(ran: Ran | null): StepPassage {
  return {
    run_id: "r-old",
    attempt: 1,
    started_at: 100,
    ended_at: 101,
    outcome: "Went",
    failure_class: null,
    refusal: null,
    ran,
    started_by: "window",
    input: {},
    mandate: null,
    signal_who: null,
    signal_where: null,
    said: null,
    output: null,
  };
}

describe("a step in its history", () => {
  test("AN OLD LINE READS THE SAME WAY, from the ledger", async () => {
    const stop = pretendShell([passageWith(SHELL_LINE)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      fireEvent.click(await screen.findByRole("button", { expanded: false }));
      expect(container.querySelector(".step-ran")?.textContent).toBe("sh -c «echo hi»");
    } finally {
      stop();
    }
  });

  test("a passage that started no line shows no such element", async () => {
    const stop = pretendShell([passageWith(null)]);
    try {
      const { container } = render(<StepHistory flowName="prova" stepId="verdict" />);
      fireEvent.click(await screen.findByRole("button", { expanded: false }));
      expect(container.querySelector(".step-ran")).toBeNull();
    } finally {
      stop();
    }
  });
});
