// @vitest-environment jsdom

// **THE PANEL CANNOT DROP A FIELD IT CANNOT READ.**
//
// What you cannot read you must not rewrite: an unknown field is left where it
// was instead of being omitted, because omitting is a write, and a silent write
// on somebody else's file is a loss nobody sees until they reopen the flow.

// THE TEST RUNS ON THE REAL FILES AND RE-READS THE FILE, not in-memory state.
// A test comparing `choice` with what the panel shows would stay green: `choice`
// is already the mutilated copy. Here a flow from `flows/` is taken as it is on
// disk, put through the panel with the gesture its users make all day — typing
// in the «Modello» field — and the step is rebuilt as it would land on disk.
// That is what gets compared.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { StepEditor } from "./StepEditor";
import { joinToolParams, splitToolParams } from "./tools";
import {
  SHIPPED_WITH_THE_BINARY,
  parseFlow,
  readRealFlows,
  shippedFlowsMissingFrom,
} from "./realflows";
import type { FlowFile, Step } from "./flow";

afterEach(cleanup);

/**
 * The flows shipped inside the binary, read through the one reader
 * `realflows.ts` owns. The panel rewrites them all alike, so this sees all.
 */
function realFlows(): Array<{ path: string; flow: FlowFile }> {
  return readRealFlows().map(({ path, source }) => ({
    path,
    flow: parseFlow<FlowFile>(path, source),
  }));
}

/** The steps of the real flows that declare an engine chain, with their file. */
function stepsWithAChain(): Array<{ path: string; step: Step }> {
  const found: Array<{ path: string; step: Step }> = [];
  for (const { path, flow } of realFlows()) {
    for (const step of flow.graph.steps) {
      if (Array.isArray(step.with?.tool)) found.push({ path, step });
    }
  }
  return found;
}

/**
 * Puts a step through the real panel and returns the `with` as it would land on
 * disk after touching the «Modello» field. It mounts the panel instead of
 * simulating it: the loss comes from how `splitToolParams`, `joinToolParams`
 * and the panel's keystrokes fit together, and a simulation would err alongside.
 */
function throughThePanel(step: Step, newModel: string): Record<string, unknown> | null {
  let written: Record<string, unknown> | null | undefined;
  render(
    <StepEditor
      flowName="prova"
      color="#000"
      step={step}
      siblingIds={[]}
      tools={[]}
      discovery={{ state: "ready", tools: [] }}
      usedModels={[]}
      onRename={() => {}}
      onField={(patch) => {
        written = patch.with;
      }}
      onToggleDep={() => {}}
      onDelete={() => {}}
    />,
  );
  const field = screen.getByLabelText("Model") as HTMLInputElement;
  expect(field, "the panel does not show the «Model» field").toBeDefined();
  fireEvent.change(field, { target: { value: newModel } });
  expect(written, "the panel wrote nothing").not.toBeUndefined();
  return written as Record<string, unknown> | null;
}

describe("the panel rewrites a step without losing what it cannot read", () => {
  const chained = stepsWithAChain();

  test("the real flows with a chain really load, and the shipped ones are among them", () => {
    // Without this, everything else would run on zero steps — the quietest way
    // to be green for having looked at nothing. A count was the wrong guard:
    // `toBe(20)` described a `flows/` directory that moved out of the repo, and
    // the test called it a defect. Named instead: what ships inside the binary,
    // read from `system.rs`. Today 2 chains, both in `dispatch-the-work`; the
    // floor is one, which is what the tests below need.
    expect(SHIPPED_WITH_THE_BINARY, "system.rs yields no include_str! line").not.toEqual([]);
    const paths = realFlows().map(({ path }) => path);
    expect(shippedFlowsMissingFrom(paths).join(", "), `read: ${paths.join(", ")}`).toBe("");
    expect(chained.length).toBeGreaterThanOrEqual(1);
  });

  test("ONE PASS THROUGH THE PANEL DOES NOT ERASE THE CHAIN FROM THE FILE", () => {
    const lost: string[] = [];
    for (const { path, step } of chained) {
      const before = step.with as Record<string, unknown>;
      const after = throughThePanel(step, "opus") ?? {};
      cleanup();
      for (const key of Object.keys(before)) {
        if (!(key in after)) {
          lost.push(
            `${path} · step «${step.id}»: «${key}» disappears — it was ${JSON.stringify(
              before[key],
            )}, and after saving the step is ${JSON.stringify(after)}`,
          );
        }
      }
    }
    expect(lost.join("\n"), `the panel erased fields:\n${lost.join("\n")}`).toBe("");
  });

  test("and the chain lands back on disk IDENTICAL, not merely present", () => {
    // «Present» is not enough: a list rewritten in another order is another
    // order of preference, that is, another engine in first place.
    for (const { path, step } of chained) {
      const after = throughThePanel(step, "opus") ?? {};
      cleanup();
      expect(after.tool, `${path} · step «${step.id}»`).toEqual(step.with?.tool);
    }
  });

  test("the model typed in the panel still reaches the file", () => {
    // The defence must not turn into «the panel writes nothing any more»: the
    // gesture being made has to keep working.
    const { step } = chained[0];
    const after = throughThePanel(step, "opus") ?? {};
    expect(after.model).toBe("opus");
  });

  test("and the panel SAYS the chain is there, instead of showing «nessuno»", () => {
    // Leaving the field where it was is not enough if the selector then reads
    // «— nessuno —» over a step that names three engines: that would be the
    // node's lie moved one window across, the only difference being that this
    // one erases nothing.
    const { step } = chained[0];
    throughThePanel(step, "opus");
    const chain = (step.with?.tool as string[]).join(" › ");
    expect(screen.getByText(new RegExp(chain))).toBeDefined();
  });

  test("picking a tool replaces the chain, because that is an explicit gesture", () => {
    // The other side: leaving a field where it was applies to whoever does NOT
    // touch it. Whoever picks «codex» in the panel has chosen, and the chain
    // gives way — otherwise the selector would be a fake.
    const { step } = chained[0];
    const { rest, choice } = splitToolParams(step.with);
    const after = joinToolParams(rest, { ...choice, tool: "codex" });
    expect(after?.tool).toBe("codex");
  });
});
