// @vitest-environment jsdom
/**
 * **A LEDGER THAT WAS NEVER WRITTEN IS NOT AN EMPTY ONE**, and a record left
 * open is not a process still running. Both distinctions vanish in a count, and
 * both change what somebody does next.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { LedgerScreen } from "./LedgerScreen";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const NOW = 2_000_000;

const HELD = {
  directory: "/home/pilot/.claude/state/flussi",
  exists: true,
  runs: 812,
  unfinished: [
    { run_id: "r-91", entity: "dispatch-the-work", open_steps: 2, oldest_started_at: NOW - 7200 },
  ],
  waiting: [],
  leftovers: [
    { process_id: "vite-5183", pid: 41221, command: "npm run dev", working_directory: "/x", port: 5183, alive: true },
    { process_id: "old-suite", pid: 3, command: "cargo test", working_directory: "/y", port: null, alive: false },
  ],
  failures: [
    { class: "timeout", failures: 7, runs_affected: 4 },
    { class: null, failures: 2, runs_affected: 2 },
  ],
  kept: [{ collection: "sailor", key: "last-handover" }],
  inventory_present: 143,
  inventory_gone: 6,
};

function answering(payload: unknown): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(payload) },
  };
}

describe("the ledger screen", () => {
  test("A LEDGER THAT IS NOT THERE SAYS SO, and still says where it looked", async () => {
    answering({ ...HELD, exists: false, runs: 0, leftovers: [], unfinished: [], failures: [], kept: [] });
    const { container } = render(<LedgerScreen native now={NOW} />);

    await waitFor(() => expect(container.textContent).toContain("no ledger there yet"));
    expect(container.textContent).toContain("/home/pilot/.claude/state/flussi");
    // Not «0 processes left running»: it has nothing to say about them at all.
    expect(container.textContent).not.toContain("never saw end");
  });

  test("A RECORD LEFT OPEN AND A PROCESS STILL RUNNING ARE TOLD APART", async () => {
    answering(HELD);
    const { container } = render(<LedgerScreen native now={NOW} />);
    await waitFor(() => expect(screen.getByText("vite-5183")).toBeTruthy());

    const states = [...container.querySelectorAll("td[data-state]")].map((c) => c.getAttribute("data-state"));
    // THE CONTROL: with one state, everything below would hold on a screen
    // that draws the two the same.
    expect(new Set(states).size, `both rows read as ${states.join(", ")}`).toBe(2);
    expect(container.textContent).toContain("still running");
    expect(container.textContent).toContain("gone");
  });

  test("A FAILURE THE ENGINE COULD NOT CLASSIFY IS NOT PUT IN A CLASS", async () => {
    answering(HELD);
    const { container } = render(<LedgerScreen native now={NOW} />);
    await waitFor(() => expect(screen.getByText("timeout")).toBeTruthy());
    expect(container.textContent).toContain("not classified");
    // And it is not folded into the named one: two rows, two counts.
    expect(container.textContent).toContain("7");
    expect(container.textContent).toContain("2");
  });

  test("A PORT NOBODY HOLDS IS SAID, not left blank", async () => {
    answering(HELD);
    const { container } = render(<LedgerScreen native now={NOW} />);
    await waitFor(() => expect(screen.getByText("old-suite")).toBeTruthy());
    const row = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.includes("old-suite"));
    expect(row?.textContent).toContain("not held");
  });

  test("AN EMPTY STORE EXPLAINS WHAT IT CANNOT SEE", async () => {
    answering({ ...HELD, kept: [] });
    const { container } = render(<LedgerScreen native now={NOW} />);
    await waitFor(() => expect(container.textContent).toContain("What flows have kept"));
    // A flow names its own collection: an empty list here is not proof that
    // nothing was written anywhere.
    expect(container.textContent).toContain("would not show up in this list");
  });
});
