// @vitest-environment jsdom
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TheWindow } from "./TheWindow";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

/** One store on the disk, one counted but weightless, one never created. */
const KEEPS = {
  home: "/home/mira/.config/sailor",
  home_files: 96,
  home_bytes: 1024 * 1024,
  stores: [
    { what: "Runs, steps and events", where: "/home/mira/.config/sailor/ledger", how_many: 204_887, bytes: 96 * 1024 * 1024, exists: true, since: 1_756_000_000 },
    { what: "Prices", where: "/home/mira/.config/sailor/pricing.json", how_many: null, bytes: 2048, exists: true, since: null },
    { what: "Faults", where: "/home/mira/.config/sailor/ledger/faults.db", how_many: null, bytes: null, exists: false, since: null },
  ],
  in_service: { binary: null, built_at: null, commit: null, window_version: "0.1.0" },
  project_root: null,
};

function machineKeeps(answer: unknown) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        if (command === "what_sailor_keeps") return Promise.resolve(answer);
        return Promise.resolve([]);
      },
    },
  };
}

/** The one case the window is drawn for: nobody answers, and why is the answer. */
function machineRefuses(why: string) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.reject(new Error(why)) },
  };
}

async function openData() {
  render(<TheWindow native />);
  fireEvent.click(screen.getByRole("button", { name: "Data" }));
}

function rows(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>(".window-row")];
}

describe("the data list draws its panel", () => {
  test("A ROW PER STORE, WITH THE ROOM IT TAKES, and the missing one says so", async () => {
    machineKeeps(KEEPS);
    await openData();
    await waitFor(() => { expect(rows()).toHaveLength(3); });
    expect(rows().map((one) => one.querySelector(".window-row__name")?.textContent)).toEqual([
      "Runs, steps and events",
      "Prices",
      "Faults",
    ]);
    expect(rows()[0].querySelector(".window-row__since")?.textContent).toBe("96.0 MB");
    expect(rows()[1].querySelector(".window-row__since")?.textContent).toBe("2 KB");
    // A store nobody has written to yet gets the words, never a plausible size.
    expect(rows()[2].querySelector(".window-row__since")).toBeNull();
    expect(rows()[2].querySelector(".window-row__gone")?.textContent).toBe("not created yet");
  });

  test("the field opens the store that was chosen, path and count and date", async () => {
    machineKeeps(KEEPS);
    await openData();
    await waitFor(() => { expect(rows()).toHaveLength(3); });
    fireEvent.click(rows()[0]);
    await screen.findByText("/home/mira/.config/sailor/ledger");
    const facts = document.querySelector(".window-facts")?.textContent ?? "";
    expect(facts).toContain("/home/mira/.config/sailor/ledger");
    // Grouped in threes, as the design writes a count: six figures in a row are
    // counted with a finger, not read.
    expect(facts).toContain("204 887");
    expect(facts).toContain("96.0 MB");
    expect(facts).toMatch(/\d{2}\/\d{2}\/\d{4}/);
  });

  test("a store that is not there is said in the field too, not zeroed", async () => {
    machineKeeps(KEEPS);
    await openData();
    await waitFor(() => { expect(rows()).toHaveLength(3); });
    fireEvent.click(rows()[2]);
    const said = await screen.findByText(/Not created yet/);
    // It answers for the whole store, so it takes the row rather than the
    // 140px label column auto-placement would squeeze it into.
    expect(said.classList.contains("window-facts__state")).toBe(true);
    expect(screen.getByText("/home/mira/.config/sailor/ledger/faults.db")).toBeTruthy();
  });

  test("A CHOSEN ROW IS WRITTEN, so leaving the list and coming back reopens it", async () => {
    machineKeeps(KEEPS);
    await openData();
    await waitFor(() => { expect(rows()).toHaveLength(3); });
    fireEvent.click(rows()[1]);
    await screen.findByText("/home/mira/.config/sailor/pricing.json");
    fireEvent.click(screen.getByRole("button", { name: "Workspaces" }));
    await waitFor(() => { expect(screen.queryByText("/home/mira/.config/sailor/pricing.json")).toBeNull(); });
    fireEvent.click(screen.getByRole("button", { name: "Data" }));
    await screen.findByText("/home/mira/.config/sailor/pricing.json");
    expect(rows()[1].getAttribute("aria-current")).toBe("true");
  });

  test("NOBODY ANSWERING IS SAID BY BOTH COLUMNS, and neither says «Looking…»", async () => {
    machineRefuses("the ledger could not be opened");
    await openData();
    await waitFor(() => {
      expect(document.querySelector(".window-panel__empty")?.textContent).toContain(
        "the ledger could not be opened",
      );
    });
    expect(document.querySelector(".window-field__empty")?.textContent).toContain(
      "the ledger could not be opened",
    );
    expect(screen.queryByText("Looking…")).toBeNull();
    expect(rows()).toHaveLength(0);
  });

  test("with the answer in hand and nothing chosen, the field asks for a choice", async () => {
    machineKeeps({ ...KEEPS, stores: [] });
    await openData();
    await waitFor(() => {
      expect(document.querySelector(".window-panel__empty")?.textContent).toBe(
        "Nothing is kept on this machine yet.",
      );
    });
    expect(document.querySelector(".window-field__empty")?.textContent).toContain("Pick a store on the left");
  });
});
