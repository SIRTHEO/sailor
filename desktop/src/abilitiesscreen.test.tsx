// @vitest-environment jsdom
/**
 * **THE PAGE THAT SAYS WHAT A THING MAY DO MUST CLAIM ONLY WHAT IT WAS TOLD.**
 * It grouped names under a family this window computes from the name, with
 * `check` as the fallback, and the engine's policies were readable nowhere.
 */
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { AbilitiesScreen, type Registered } from "./AbilitiesScreen";
import { KNOWN_ACTIONS } from "./flow";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

function answering(actions: Registered[]): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(actions) },
  };
}

/** An action taking every default, as the engine reports it. */
function silent(name: string): Registered {
  return { name, redo: "hand_to_human", closes_a_run: false };
}

function rowOf(container: HTMLElement, name: string): string {
  const row = [...container.querySelectorAll("tbody tr")].find(
    (one) => one.querySelector("code")?.textContent === name,
  );
  return row?.textContent ?? "";
}

describe("what Sailor can do", () => {
  test("EVERY ROW CARRIES THE ENGINE'S POLICY, and the policy is the engine's to vary", async () => {
    answering([
      silent(KNOWN_ACTIONS[0]),
      { name: KNOWN_ACTIONS[1], redo: "repeatable", closes_a_run: true },
      { name: KNOWN_ACTIONS[2], redo: "compensable", closes_a_run: false },
    ]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(rowOf(container, KNOWN_ACTIONS[0])).not.toBe(""));
    // An action that answered nothing reads as the engine's cautious default.
    expect(rowOf(container, KNOWN_ACTIONS[0])).toContain("hand it to a person");
    expect(rowOf(container, KNOWN_ACTIONS[1])).toContain("run it again");
    expect(rowOf(container, KNOWN_ACTIONS[2])).toContain("undo, then run it again");
    // The control: one word printed for all three would pass every line above.
    expect(rowOf(container, KNOWN_ACTIONS[1])).not.toContain("hand it to a person");
  });

  test("A WORD THIS VERSION HAS NO NAME FOR TRAVELS WHOLE, never as a default", async () => {
    answering([{ name: KNOWN_ACTIONS[0], redo: "some_new_policy", closes_a_run: false }]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(rowOf(container, KNOWN_ACTIONS[0])).not.toBe(""));
    expect(rowOf(container, KNOWN_ACTIONS[0])).toContain("some_new_policy");
    expect(rowOf(container, KNOWN_ACTIONS[0])).not.toContain("hand it to a person");
  });

  test("AN ACTION WITH NO NODE IS NAMED, and named as a drawing and not a power", async () => {
    answering([silent("a_brand_new_action"), silent(KNOWN_ACTIONS[0])]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(rowOf(container, "a_brand_new_action")).not.toBe(""));
    expect(rowOf(container, "a_brand_new_action")).toContain("no node of its own");
    // The control: a page that flagged every row would pass the line above.
    expect(rowOf(container, KNOWN_ACTIONS[0])).not.toContain("no node of its own");
  });

  /** On a page about what an action may do, an unseen absence reads as «none». */
  test("WHAT NOBODY DECLARES IS SAID, not left as a blank to be read as «none»", async () => {
    answering([silent(KNOWN_ACTIONS[0])]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(rowOf(container, KNOWN_ACTIONS[0])).not.toBe(""));
    const found = container.querySelector(".now__why");
    expect(found, "the page says nothing about what nobody declares").not.toBeNull();
    const said = found as HTMLElement;
    const named = [...said.querySelectorAll("code")].map((one) => one.textContent);
    expect(named).toEqual(["sense", "act", "remember", "gate", "docs/the-four-surfaces.md"]);
    expect(said.textContent).toContain("fault 67");
    expect(said.textContent, "the page leaves money to be read as a fact about an action").toContain(
      "not a fact about an action",
    );
    // And no row pretends to carry one: an inferred surface is fault 30 here.
    expect(rowOf(container, KNOWN_ACTIONS[0])).not.toContain("sense");
  });

  test("THE LIST IS THE ENGINE'S, so an empty answer is shown as empty", async () => {
    answering([]);
    const { container } = render(<AbilitiesScreen native />);
    await waitFor(() => expect(container.textContent).toContain("What Sailor can do"));
    expect(container.querySelectorAll("tbody tr").length).toBe(0);
  });

  test("OUTSIDE THE SHELL IT SAYS SO", () => {
    const { container } = render(<AbilitiesScreen native={false} />);
    expect(container.textContent).toContain("I cannot ask the engine");
  });
});
