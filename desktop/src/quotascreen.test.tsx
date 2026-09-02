// @vitest-environment jsdom
/**
 * **A QUOTA THAT COULD NOT BE READ IS NOT A QUOTA OF ZERO**, and a price the
 * catalogue does not carry is not a price of nothing. Both mistakes point the
 * same way — the reassuring one — which is why they are guarded here.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { QuotaScreen } from "./QuotaScreen";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const NOW = 1_000_000;

const WINDOWS = [
  { engine: "claude", unit: "five_hour", spent_fraction: 0.614, resets_at: "2026-09-02T18:00:00Z", observed_at: NOW - 120 },
  { engine: "claude", unit: "thirty_day", spent_fraction: 1, resets_at: null, observed_at: NOW - 120 },
];

const CATALOGUE = {
  models: [
    { id: "a/free-one:free", name: "Free One", free: true, context_length: 128_000, price_in: 0, price_out: 0, modalities: ["text"] },
    { id: "b/cheap", name: "Cheap", free: false, context_length: 200_000, price_in: 0.0015, price_out: 0.006, modalities: ["text", "image"] },
    { id: "c/unpriced", name: "Unpriced", free: false, context_length: null, price_in: null, price_out: null, modalities: ["text"] },
  ],
  choices: [
    { kind: "default", chosen: "b/cheap", in_force: "a/free-one:free" },
    { kind: "notte", chosen: null, in_force: "a/free-one:free" },
  ],
};

function engine(answers: Record<string, unknown>, fails: Record<string, string> = {}): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) =>
        command in fails
          ? Promise.reject(new Error(fails[command]))
          : Promise.resolve(answers[command]),
    },
  };
}

describe("the quota screen", () => {
  test("A CHANNEL THAT DOES NOT ANSWER SAYS SO, and shows no number", async () => {
    engine({ models_catalogue: CATALOGUE }, { quota: "the engine refused: the token has been revoked" });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(container.textContent).toContain("I could not read it"));
    // The provider's own words say what to do, so they travel whole.
    expect(container.textContent).toContain("token has been revoked");
    // AND NO BAR IS DRAWN: an empty bar reads as «nothing spent», which is the
    // one conclusion that must not be reachable from a failed reading.
    expect(container.querySelectorAll(".quota__bar").length, "a bar was drawn for a quota nobody read").toBe(0);
  });

  test("EVERY WINDOW IS SHOWN, including one this version has no name for", async () => {
    engine({ quota: WINDOWS, models_catalogue: CATALOGUE });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(screen.getByText("5 hours")).toBeTruthy());
    expect(container.querySelectorAll(".quota__bar").length, "a window went missing").toBe(2);
    // The unknown one appears under its own key rather than being dropped.
    expect(container.textContent).toContain("thirty day");
    // A reading carries when it was taken, or it cannot be told from yesterday's.
    expect(container.textContent).toContain("2 min ago");
  });

  test("WHAT WAS CONFIGURED AND WHAT RUNS ARE SHOWN APART", async () => {
    engine({ quota: WINDOWS, models_catalogue: CATALOGUE });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(container.textContent).toContain("default"));
    // The saved choice points at a paid model; the engine overrules it. A
    // screen showing only the wish would explain nothing when a run differs.
    expect(container.textContent).toContain("configured but not in force");
  });

  test("A PRICE THE CATALOGUE DOES NOT CARRY IS NOT FREE", async () => {
    engine({ quota: WINDOWS, models_catalogue: CATALOGUE });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(screen.getByText("Unpriced")).toBeTruthy());
    const row = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.includes("Unpriced"));
    expect(row?.textContent, "an unpriced model reads as free").toContain("no price");
    expect(row?.textContent, "and its context is not invented either").toContain("not stated");

    // The control: the really-free one does say free, or the check above would
    // pass on a screen that never says it.
    const free = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.includes("Free One"));
    expect(free?.textContent).toContain("free");
  });

  test("ONLY A FREE MODEL IS OFFERED, because that is the engine's rule", async () => {
    engine({ quota: WINDOWS, models_catalogue: CATALOGUE });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(screen.getByText("Free One")).toBeTruthy());
    const buttons = [...container.querySelectorAll("button")].filter((b) => b.textContent?.includes("use for default"));
    expect(buttons.length, "the offer is not on exactly the free models").toBe(1);
    const row = buttons[0].closest("tr");
    expect(row?.textContent, "the offer landed on a paid model").toContain("Free One");
  });
});
