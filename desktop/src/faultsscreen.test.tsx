// @vitest-environment jsdom
/**
 * **A WORDING NOBODY TAUGHT THE REGISTER IS NOT A CLOSED FAULT.** The fourth
 * standing exists so a fault cannot leave the open count by an edit nobody
 * meant as one, and a screen that hid it would undo that at the last step.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { FaultsScreen } from "./FaultsScreen";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const BOOK = {
  path: "/home/pilot/.config/sailor/faults.json",
  still_open: 2,
  entries: [
    { number: 12, happened_on: "2026-08-30", what_happened: "a list could not say it had not looked",
      how_it_showed: "zero read as «none»", what_would_prevent: "a third state, carried with its reason",
      status: "**aperto**", standing: "open" },
    { number: 8, happened_on: "2026-08-24", what_happened: "unknown fields were dropped on save",
      how_it_showed: "a marker lost a key nobody here wrote", what_would_prevent: "keep what you did not understand",
      status: "**chiuso**", standing: "closed" },
    { number: 41, happened_on: "2026-09-01", what_happened: "a design element the engine did not support",
      how_it_showed: "a mockup that could not be built", what_would_prevent: "",
      status: "mezzo sistemato, credo", standing: "unrecognised" },
  ],
};

function answering(book: unknown = BOOK): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(book) },
  };
}

describe("the faults screen", () => {
  test("AN UNRECOGNISED STANDING STAYS IN THE OPEN VIEW, and says why", async () => {
    answering();
    const { container } = render(<FaultsScreen native />);
    await waitFor(() => expect(screen.getByText(/a list could not say/)).toBeTruthy());

    // The default view hides only what is really closed.
    expect(container.textContent, "a closed fault is in the open view").not.toContain("unknown fields were dropped");
    // THE ONE THAT MATTERS: neither open nor closed, so it must not vanish.
    expect(container.textContent).toContain("a design element the engine did not support");
    expect(container.textContent).toContain("does not recognise this wording");
  });

  test("AN ENTRY WITH NO PREVENTION IS MARKED, not drawn as finished", async () => {
    answering();
    const { container } = render(<FaultsScreen native />);
    await waitFor(() => expect(container.textContent).toContain("what would prevent it"));
    expect(container.textContent).toContain("this entry is not finished");

    // The control: an entry that has one shows it instead of the warning.
    expect(container.textContent).toContain("a third state, carried with its reason");
  });

  test("THE GESTURE OFFERS THE REGISTER'S OWN WORDS, and not the one already set", async () => {
    answering();
    const { container } = render(<FaultsScreen native />);
    await waitFor(() => expect(screen.getByText(/a list could not say/)).toBeTruthy());

    const open = [...container.querySelectorAll(".panel__block")]
      .find((block) => block.textContent?.includes("a list could not say"));
    const offers = [...(open?.querySelectorAll("button") ?? [])].map((b) => b.textContent);
    // Three words in the register, minus the one this fault already has.
    expect(offers).toEqual(["mark partly closed", "mark closed"]);
  });

  test("WHERE THE REGISTER IS, said out loud", async () => {
    answering();
    const { container } = render(<FaultsScreen native />);
    await waitFor(() => expect(container.textContent).toContain("faults.json"));
  });

  test("OUTSIDE THE SHELL IT SAYS SO", () => {
    const { container } = render(<FaultsScreen native={false} />);
    expect(container.textContent).toContain("I cannot read the register");
    expect(container.querySelectorAll(".panel__block").length).toBe(0);
  });
});
