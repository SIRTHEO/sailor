// @vitest-environment jsdom
/**
 * **THE MODELS PAGE ANSWERS «WHAT WILL THE WORK RUN ON, AND WHAT MAY I PICK
 * INSTEAD».** It used to be the spend screen: a quota first, then a price list
 * of hundreds nobody may choose, and what `default` runs on as a bare id.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { ModelsScreen, effectiveFor, inOrder } from "./ModelsScreen";
import { SailorScreen } from "./SailorScreen";
import type { Priced } from "./quota";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const CATALOGUE = {
  models: [
    { id: "b/cheap", name: "Cheap", free: false, context_length: 200_000, price_in: 0.0015, price_out: 0.006, modalities: ["text", "image"] },
    { id: "c/unpriced", name: "Unpriced", free: false, context_length: null, price_in: null, price_out: null, modalities: ["text"] },
    { id: "a/free-one:free", name: "Free One", free: true, context_length: 128_000, price_in: 0, price_out: 0, modalities: ["text"] },
  ],
  choices: [
    { kind: "default", chosen: "b/cheap", in_force: "a/free-one:free" },
    { kind: "notte", chosen: null, in_force: "a/free-one:free" },
  ],
};

function engine(answers: Record<string, unknown>, fails: Record<string, string> = {}): string[] {
  const asked: string[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        asked.push(command);
        return command in fails
          ? Promise.reject(new Error(fails[command]))
          : Promise.resolve(answers[command]);
      },
    },
  };
  return asked;
}

describe("the models page", () => {
  test("IT LEADS WITH WHAT THE WORK RUNS ON, NOT WITH A QUOTA", async () => {
    const asked = engine({ models_catalogue: CATALOGUE });
    const { container } = render(<ModelsScreen native />);

    await waitFor(() => expect(screen.getByText("What each kind of work runs on")).toBeTruthy());
    const blocks = Array.from(container.querySelectorAll(".panel__title")).map((one) => one.textContent ?? "");
    expect(blocks[0], "something stands before the setting").toBe("What each kind of work runs on");
    expect(blocks[1]).toMatch(/^Choose a free model/);
    // The quota is another page's question, and this page never asks for it.
    expect(asked).toEqual(["models_catalogue"]);
    expect(container.querySelectorAll(".quota__bar"), "a quota bar on the models page").toHaveLength(0);
  });

  /**
   * The two rows advertised different questions and mounted the same
   * component: «Models» was the spend screen, and «which is in use» existed
   * nowhere.
   */
  test("AND THE ROW NAMED «MODELS» OPENS IT, not the quota screen", async () => {
    engine({ models_catalogue: CATALOGUE });
    const { container } = render(<SailorScreen native tab="models" />);

    await waitFor(() => expect(screen.getByText("What each kind of work runs on")).toBeTruthy());
    expect(container.textContent, "the models row still opens the quota").not.toContain("Quota already spent");
  });

  test("THE EFFECTIVE MODEL IS NAMED, and the overruled wish is named beside it", async () => {
    engine({ models_catalogue: CATALOGUE });
    const { container } = render(<ModelsScreen native />);

    await waitFor(() => expect(screen.getByText("What each kind of work runs on")).toBeTruthy());
    const row = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.startsWith("default"));
    // A NAME, NOT ONLY AN ID: the id stays as the thing you paste, under it.
    expect(row?.textContent).toContain("Free One");
    expect(row?.textContent).toContain("a/free-one:free");
    // The configured paid model is hidden from the list below, so its name has
    // to survive here or it is nowhere at all.
    expect(row?.textContent).toContain("Cheap");
    expect(row?.textContent).toContain("the free-only rule overrules it");
    // And nothing on the page claims this is what a run actually called.
    expect(container.textContent).toContain("not what a run really called");
  });

  test("THE LIST OFFERS THE FREE ONES AND SAYS SO, and marks the one in force", async () => {
    engine({ models_catalogue: CATALOGUE });
    const { container } = render(<ModelsScreen native />);

    await waitFor(() => expect(container.textContent).toContain("1 free to choose · 3 in the catalogue"));
    const offered = () => [...container.querySelectorAll(".now__entity")].map((one) => one.textContent ?? "");
    expect(offered().some((row) => row.startsWith("Cheap")), "a paid model is offered unasked").toBe(false);
    const marks = [...container.querySelectorAll(".now__badge")].map((one) => one.textContent);
    expect(marks).toEqual(["effective for default", "effective for notte"]);

    // Asked for, the paid ones come back — and they carry why they cannot be
    // picked, instead of a blank cell that explains nothing.
    fireEvent.click(screen.getByRole("checkbox"));
    await waitFor(() => expect(offered().some((row) => row.startsWith("Cheap"))).toBe(true));
    const paid = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.startsWith("Cheap"));
    expect(paid?.textContent).toContain("paid: the free-only rule refuses it");
    // A price the catalogue does not carry is still not a price of nothing.
    const unpriced = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.includes("Unpriced"));
    expect(unpriced?.textContent).toContain("no price");
    expect(unpriced?.textContent).toContain("not stated");
  });

  test("AN EMPTY SEARCH OVER THE FREE ONES IS NOT AN EMPTY CATALOGUE", async () => {
    engine({ models_catalogue: CATALOGUE });
    const { container } = render(<ModelsScreen native />);

    await waitFor(() => expect(container.textContent).toContain("1 free to choose"));
    fireEvent.change(screen.getByPlaceholderText("search by name or id"), { target: { value: "cheap" } });
    expect(screen.getByText(/Paid models are hidden/)).toBeTruthy();
  });

  test("ONE FAILED READ IS NOT A MACHINE WITH NOTHING CONFIGURED", async () => {
    engine({}, { models_catalogue: "the catalogue could not be fetched" });
    const { container } = render(<ModelsScreen native />);

    await waitFor(() => expect(container.textContent).toContain("Neither the settings nor the catalogue"));
    expect(container.textContent).toContain("the catalogue could not be fetched");
    // NOT «no model»: an empty settings table would read as a machine that has
    // nothing configured, which is a different fact from one nobody could read.
    expect(container.querySelectorAll("table"), "an empty table stood for an unread one").toHaveLength(0);
    expect(screen.getByRole("button", { name: "ask again" })).toBeTruthy();
  });

  test("THE MARKED ROWS COME FIRST, so the cut cannot hide them", () => {
    const many: Priced[] = Array.from({ length: 3 }, (_, index) => ({
      id: `m${index}`, name: `M${index}`, free: true, context_length: null,
      price_in: 0, price_out: 0, modalities: [],
    }));
    const effective = effectiveFor([{ kind: "default", chosen: null, in_force: "m2" }]);
    expect(inOrder(many, effective).map((one) => one.id)).toEqual(["m2", "m0", "m1"]);
    // The control: with nothing in force the order is the catalogue's own.
    expect(inOrder(many, effectiveFor([])).map((one) => one.id)).toEqual(["m0", "m1", "m2"]);
  });
});
