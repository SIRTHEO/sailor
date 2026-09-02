// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { liveWords, spendWords, whoWords, LiveChip } from "./Bar";
import { LedgerBrowser } from "./LedgerBrowser";
import { PLACES } from "./Rail";

/**
 * **FOUR PLACES, A BAR THAT SPEAKS FROM ANYWHERE, AND THE LEDGER AS A
 * DATABASE.** The window used to have five doors in a row and ten more behind
 * one of them; the bar knew about a run only on the board; the ledger answered
 * eight fixed questions. Each of these is now a thing a test can turn red.
 */

afterEach(cleanup);

/** Enough for React Flow to mount: the canvas is not the subject here. */
class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

function pretendShell(answers: Record<string, unknown | ((args?: Record<string, unknown>) => unknown)>) {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  const calls: Call[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        if (!(command in answers)) return Promise.reject(new Error(`the fake shell has no ${command}`));
        const answer = answers[command];
        const value = typeof answer === "function" ? (answer as (args?: Record<string, unknown>) => unknown)(args) : answer;
        if (value instanceof Error) return Promise.reject(value);
        return Promise.resolve(value);
      },
    },
  };
  return {
    calls,
    stop: () => {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    },
  };
}

describe("the four places", () => {
  test("THE COLUMN HAS THREE GROUPS AND FOUR PLACES, each with its question, and opens on the board", () => {
    const { container } = render(<App />);
    const headings = Array.from(container.querySelectorAll(".places__heading")).map((one) => one.textContent);
    expect(headings).toEqual(["work", "what happened", "itself"]);
    const names = Array.from(container.querySelectorAll(".places__name")).map((one) => one.textContent);
    expect(names).toEqual(["Board", "Terminals", "Memory", "Sailor"]);
    for (const place of PLACES) {
      expect(screen.getByText(place.asks)).toBeTruthy();
    }
    expect(container.querySelector(".places__item[data-here] .places__name")?.textContent).toBe("Board");
    expect(container.querySelector(".body[hidden]"), "the board is not in view at rest").toBeNull();
  });

  test("THE BAR SAYS WHERE YOU ARE: the place, then the entry inside it", () => {
    const { container } = render(<App />);
    const crumbs = () => Array.from(container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);
    expect(crumbs()).toEqual(["Board"]);

    fireEvent.click(screen.getByRole("button", { name: /^Memory/ }));
    expect(crumbs()).toEqual(["Memory", "Runs"]);
    // The terminals stay mounted, hidden, with a column of their own: only
    // the section in view counts.
    const shown = ".section:not([hidden]) ";
    const entries = Array.from(container.querySelectorAll(`${shown}.subrail__name`)).map((one) => one.textContent);
    expect(entries).toEqual(["Runs", "Ledger", "Spend and quota", "Faults"]);

    fireEvent.click(screen.getByRole("button", { name: /^Ledger/ }));
    expect(crumbs()).toEqual(["Memory", "Ledger"]);

    fireEvent.click(screen.getByRole("button", { name: /^Sailor/ }));
    expect(crumbs()).toEqual(["Sailor", "What it can do"]);
    const groups = Array.from(container.querySelectorAll(`${shown}.subrail .places__heading`)).map((one) => one.textContent);
    expect(groups).toEqual(["itself", "setup"]);

    fireEvent.click(screen.getByRole("button", { name: /^Terminals/ }));
    expect(crumbs()).toEqual(["Terminals", "Live"]);
  });
});

describe("the bar's three facts", () => {
  const working = {
    run_id: "relay-1",
    entity: "relay",
    state: "working" as const,
    open_steps: 1,
    open_now: [{ step_id: "write-the-baton", attempt: 1, open_for_secs: 30 }],
    since: 1000,
    started_here: true,
  };

  test("what runs is said from the open runs, and nothing running is said as such", () => {
    // The absurd control first: no run, no invented run.
    expect(liveWords([], 1700)).toEqual({ live: false, word: "nothing running" });
    const said = liveWords([working], 1000 + 12 * 60);
    expect(said.live).toBe(true);
    expect(said.word).toBe("relay · at write-the-baton · 12m");
    const waiting = liveWords([{ ...working, state: "waiting", open_now: [] }], 1030);
    expect(waiting.live).toBe(false);
    expect(waiting.word).toBe("relay · 30s · waiting for you");
    expect(liveWords([working, { ...working, run_id: "x", entity: "night" }], 1005).word).toContain("+1 more");
  });

  test("what it cost is a floor when a call had no price, and says so", () => {
    const summary = {
      ledger_present: true,
      runs: 4,
      went: 3,
      broke: 1,
      still_open: 0,
      input_tokens: 0,
      output_tokens: 0,
      cached_tokens: 0,
      cache_write_tokens: 0,
      cost_micros: 340_000,
      unmeasured: 0,
      unpriced: 0,
      tokens_by_model: {},
    };
    expect(spendWords(summary)).toBe("$0.34 today");
    expect(spendWords({ ...summary, unpriced: 3 })).toBe("$0.34 today (a floor)");
    expect(spendWords({ ...summary, ledger_present: false })).toBe("no ledger yet");
    expect(spendWords(null)).toBe("");
  });

  test("who you work as is the active profile of each command line", () => {
    const row = { cli_id: "codex", name: "prove", home_dir: "/h", active: true, access: "yes" as const, said: "" };
    expect(whoWords([])).toBe("no profile active");
    expect(whoWords([row, { ...row, cli_id: "claude", name: "work" }, { ...row, name: "old", active: false }])).toBe(
      "codex prove · claude work",
    );
  });

  test("THE CHIP READS THE ENGINE, from any place: open_runs and day_summary are asked", async () => {
    const shell = pretendShell({ open_runs: [working], day_summary: { ledger_present: true, cost_micros: 340_000, unpriced: 0 } });
    try {
      render(<LiveChip native now={1000 + 60} />);
      await screen.findByText(/relay · at write-the-baton · 1m/);
      expect(screen.getByText("$0.34 today")).toBeTruthy();
      expect(shell.calls.map((call) => call.command)).toEqual(expect.arrayContaining(["open_runs", "day_summary"]));
    } finally {
      shell.stop();
    }
  });
});

describe("the ledger as a database", () => {
  const TABLES = {
    directory: "/home/theo/.config/sailor/ledger",
    exists: true,
    tables: [
      { name: "runs", rows: 2 },
      { name: "inventory_items", rows: 310 },
    ],
  };

  test("THE TABLES ARE THE ENGINE'S, a table opens with its statement, and the picked row is laid out whole", async () => {
    const shell = pretendShell({
      ledger_tables: TABLES,
      ledger_query: (args?: Record<string, unknown>) => {
        if (String(args?.sql).includes("inventory_items")) {
          return {
            columns: ["item_id", "kind", "gone_at"],
            rows: [
              ["cli:claude", "cli", null],
              ["cli:gemini", "cli", "2026-08-30"],
            ],
            truncated: true,
          };
        }
        return new Error("no such table: nowhere");
      },
    });
    try {
      const { container } = render(<LedgerBrowser native />);
      await screen.findByText("inventory_items");
      expect(screen.getByText("310")).toBeTruthy();
      expect(screen.getByText(TABLES.directory)).toBeTruthy();

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /inventory_items/ }));
      });
      await screen.findByText("cli:gemini");
      expect(shell.calls.find((call) => call.command === "ledger_query")?.args).toEqual({
        sql: "select * from inventory_items order by 1 desc limit 200",
      });
      expect(screen.getByText(/Cut at the limit/)).toBeTruthy();
      // A null is shown as the word, in the engine's own absence, never as an empty cell.
      expect(container.querySelectorAll("td[data-null]")).toHaveLength(1);

      await act(async () => {
        fireEvent.click(screen.getByText("cli:gemini"));
      });
      const pairs = Array.from(container.querySelectorAll(".browser__pair")).map(
        (pair) => `${pair.querySelector("dt")?.textContent}=${pair.querySelector("dd")?.textContent}`,
      );
      expect(pairs).toEqual(["item_id=cli:gemini", "kind=cli", "gone_at=2026-08-30"]);

      // A statement the engine refuses is shown in the engine's words.
      const box = screen.getByLabelText("a statement for the ledger") as HTMLInputElement;
      fireEvent.change(box, { target: { value: "select * from nowhere" } });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Run" }));
      });
      await screen.findByText(/no such table: nowhere/);
    } finally {
      shell.stop();
    }
  });

  test("a missing ledger is said, not shown as an empty database", async () => {
    const shell = pretendShell({ ledger_tables: { directory: "/nowhere/ledger", exists: false, tables: [] } });
    try {
      render(<LedgerBrowser native />);
      await screen.findByText(/No ledger at \/nowhere\/ledger/);
    } finally {
      shell.stop();
    }
  });
});
