// @vitest-environment jsdom
/**
 * **AN ACTION THE CANVAS CANNOT DRAW IS NOT AN ORDINARY ONE.** `kindOf` falls
 * back to `check`, so an unknown one would sit among the checks looking like
 * any of them, and a flow using it would draw as something it is not.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { AbilitiesScreen } from "./AbilitiesScreen";
import { KNOWN_ACTIONS } from "./flow";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

function answering(actions: string[]): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(actions) },
  };
}

describe("what Sailor can do", () => {
  test("AN ACTION WITH NO NODE IS NAMED, not filed among the checks", async () => {
    answering([...KNOWN_ACTIONS, "a_brand_new_action"]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(screen.getByText("a_brand_new_action")).toBeTruthy());
    expect(container.textContent).toContain("have no node on the canvas yet");
    expect(container.textContent).toContain("no node for this one");
  });

  test("WITH EVERY ACTION KNOWN, NOTHING IS FLAGGED", async () => {
    // The control: without this, the check above would pass on a screen that
    // flags everything, which says nothing.
    answering([...KNOWN_ACTIONS]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(container.textContent).toContain("What Sailor can do"));
    expect(container.textContent).not.toContain("no node for this one");
  });

  test("THE FAMILIES ARE THE CANVAS'S OWN, and each says what it is for", async () => {
    answering([...KNOWN_ACTIONS]);
    const { container } = render(<AbilitiesScreen native />);

    await waitFor(() => expect(container.textContent).toContain("What Sailor can do"));
    const blocks = container.querySelectorAll(".panel__block");
    expect(blocks.length, "the actions were not grouped at all").toBeGreaterThan(2);
    // A family heading with no line saying what it is for is a word, not an
    // answer — the whole reason the list is grouped.
    for (const block of blocks) {
      expect(block.querySelector(".rail__note")?.textContent?.trim() ?? "").not.toBe("");
    }
  });

  test("THE LIST IS THE ENGINE'S, so an empty answer is shown as empty", async () => {
    answering([]);
    const { container } = render(<AbilitiesScreen native />);
    await waitFor(() => expect(container.textContent).toContain("What Sailor can do"));
    expect(container.querySelectorAll(".panel__block").length).toBe(0);
  });

  test("OUTSIDE THE SHELL IT SAYS SO", () => {
    const { container } = render(<AbilitiesScreen native={false} />);
    expect(container.textContent).toContain("I cannot ask the engine");
  });
});
