// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test } from "vitest";
import App from "./App";
import { beatWords, buildWords, hears, liveWords, spendWords, whoWords, LiveChip } from "./Bar";
import { BlankCanvas } from "./BlankCanvas";
import { LedgerBrowser } from "./LedgerBrowser";
import { MACHINE_GROUND, nameOfPlace, MACHINE, TERMINALS_GROUND, machineHolds, tabsThatExist } from "./places";

/** Read, never spelled: a hard-coded name breaks on a rename. */
const WHY = nameOfPlace("memory");

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

/** Picks the row ⌘K draws with exactly this label: the accessible name of an
 *  option carries the hint too, and «Runs» is a place and a view inside it. */
/* A SECTION IS A DYNAMIC IMPORT AWAY. It is fetched when the place asks for
   it, so a query fired in the same tick as the gesture finds the gap the
   fallback leaves and concludes the section is not there. */
async function theSectionArrives(): Promise<void> {
  await waitFor(() => {
    expect(document.querySelector(".section:not([hidden])")).toBeTruthy();
  });
}

async function typeInThePalette(label: string): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
  const rows = Array.from(document.querySelectorAll<HTMLElement>(".palette__entry"));
  const row = rows.find((one) => one.querySelector(".palette__label")?.textContent === label);
  expect(row, `the palette does not offer «${label}»`).toBeDefined();
  fireEvent.click(row as HTMLElement);
  await theSectionArrives();
}

describe("the column is the world", () => {
  /** A list of places says what the program has, not where the work is. */
  test("THE COLUMN IS THE TREE YOU STAND IN, and nothing above the work", () => {
    const { container } = render(<App />);

    const heads = Array.from(container.querySelectorAll(".world__head")).map(
      (one) => one.textContent,
    );
    expect(heads).toEqual(["workspaces", "flows everywhere", "outside every workspace"]);
    expect(
      container.querySelector(".body[hidden]"),
      "the board is the ground again: at rest the window is the work",
    ).not.toBeNull();
  });

  /** With no tree open the board is still reachable, out here. */
  test("WITH NO TREE OPEN THE BOARD IS STILL REACHABLE, out here", () => {
    const { container } = render(<App />);
    const outThere = container.querySelectorAll(".world__head")[1];
    const board = screen.getByRole("button", { name: /^Board/ });

    expect(board, "no way to reach the board").toBeTruthy();
    expect(
      outThere.compareDocumentPosition(board) & Node.DOCUMENT_POSITION_FOLLOWING,
      "the board is not under «outside every workspace»",
    ).toBeTruthy();
  });

  test("THE BAR SAYS WHERE YOU ARE: the place, then the entry inside it", async () => {
    const { container } = render(<App />);
    const crumbs = () => Array.from(container.querySelectorAll(".topbar__crumb")).map((one) => one.textContent);
    // A fresh window opens on what waits; the work is one crumb away, and the
    // entry inside it is the view.
    await typeInThePalette("Live");
    expect(crumbs()).toEqual([TERMINALS_GROUND, "Live"]);

    // The board opens on a flow, so the entry inside the place is that flow:
    // «Board» alone was the old at-rest state, where the paper held every flow
    // at once and none of them was the one you were in.
    fireEvent.click(screen.getByRole("button", { name: /^Board/ }));
    expect(crumbs()).toEqual(["Board", "prima-corsa"]);

    await typeInThePalette(WHY);
    expect(crumbs()).toEqual([WHY, "Runs"]);
    // The terminals stay mounted, hidden, with a column of their own: only
    // the section in view counts.
    const shown = ".section:not([hidden]) ";
    const entries = Array.from(container.querySelectorAll(`${shown}.subrail__name`)).map((one) => one.textContent);
    expect(entries).toEqual(["Runs", "Spend and quota", "Faults", "Ledger"]);

    // The ledger is one view of what happened: consulted beside the runs it
    // came from, and reached from the machine's own row without a run first.
    await typeInThePalette("Ledger");
    expect(crumbs()).toEqual([WHY, "Ledger"]);

    // ONE ENTRY, NOT TWO: a row of the machine's ground lands on the screen
    // itself, not on a list that asks again.
    await typeInThePalette("Profiles");
    expect(crumbs()).toEqual([MACHINE_GROUND, "Profiles"]);
    expect(
      container.querySelectorAll(`${shown}.subrail`),
      "the screen kept a column of its own, so the window offers one list twice",
    ).toHaveLength(0);

    // And back to the work, which is typed for rather than navigated to.
    await typeInThePalette("Live");
    expect(crumbs()).toEqual([TERMINALS_GROUND, "Live"]);
  });
});

describe("the bar's three facts", () => {
  const working = {
    run_id: "relay-1",
    entity: "relay",
    state: "working" as const,
    open_steps: 1,
    open_now: [{ step_id: "write-the-baton", attempt: 1, open_for_secs: 30, holder: "alive" as const }],
    since: 1000,
    started_here: true,
    steps_done: 3,
    steps_total: 7,
  };

  test("what runs is said from the open runs, and nothing running is said as such", () => {
    // The absurd control first: no run, no invented run.
    expect(liveWords([], 1700)).toEqual({ live: false, word: "nothing running" });
    const said = liveWords([working], 1000 + 12 * 60);
    expect(said.live).toBe(true);
    expect(said.word).toBe("relay · 3 of 7 · at write-the-baton · 12m");
    const waiting = liveWords([{ ...working, state: "waiting", open_now: [] }], 1030);
    expect(waiting.live).toBe(false);
    expect(waiting.word).toBe("relay · 3 of 7 · 30s · waiting for you");
    // A flow that cannot be read back gives no total, and no total is invented.
    expect(liveWords([{ ...working, steps_total: null }], 1030).word).toBe("relay · at write-the-baton · 30s");
    expect(liveWords([working, { ...working, run_id: "x", entity: "night" }], 1005).word).toContain("+1 more");
  });

  test("the build under the window is silent when fine, and loud when what you see is old", () => {
    expect(buildWords(null, 100)).toBeNull();
    expect(buildWords({ state: "running", message: "", changed_at: 1, running_since: 1 }, 100)).toBeNull();
    expect(buildWords({ state: "building", message: "", changed_at: 1, running_since: 1 }, 100)).toEqual({
      warn: false,
      word: "rebuilding the window…",
    });
    const failed = buildWords({ state: "build_failed", message: "error[E0308]", changed_at: 90, running_since: 100 - 15 * 60 }, 100);
    expect(failed?.warn).toBe(true);
    expect(failed?.word).toBe("REBUILD FAILED · you see the last good version running since 15m ago");
  });

  test("A BUILD THAT IS WAITING IS NOT A WARNING: nothing is wrong, and nothing moves", () => {
    // The window on the screen is the one before this build, deliberately: it
    // holds the pane being typed in and the run being watched.
    const ready = buildWords({ state: "ready", message: "", changed_at: 99, running_since: 100 - 15 * 60 }, 100);
    expect(ready?.warn).toBe(false);
    expect(ready?.word).toBe("a new build is waiting running since 15m ago");
  });

  test("A READER OF THE BAR HEARS WHAT IT ASKED FOR, and a run's chatter is not it", () => {
    // The read behind «who you run as» starts a command per profile, and a
    // run puts hundreds of facts a second on this channel.
    expect(hears("run", ["run", "beat"])).toBe(true);
    expect(hears("beat", ["run", "beat"])).toBe(true);
    expect(hears("run", ["profile"])).toBe(false);
    expect(hears("terminal", ["profile"])).toBe(false);
    expect(hears("profile", ["profile"])).toBe(true);
    // No list is every kind: a reader that did not choose keeps what it had.
    expect(hears("whatever-comes-next", undefined)).toBe(true);
  });

  test("THE BEAT IS SILENT WHILE THE SCHEDULE IS KEPT, and loud when it is not", () => {
    // Most decisions are «not due»: a chip that recited them would be a chip
    // nobody reads on the day it matters.
    const kept = { at: 1_000, decisions: [{ flow: "notte", verdict: "held" as const, why: "not due" }] };
    expect(beatWords(kept, 1_030)).toBeNull();
    expect(beatWords(null, 1_030)).toBeNull();

    // A due flow that would not start is nowhere else in the window.
    const broke = {
      at: 1_000,
      decisions: [
        { flow: "notte", verdict: "held" as const, why: "not due" },
        { flow: "relay", verdict: "broke" as const, why: "the engine is not signed in" },
      ],
    };
    expect(beatWords(broke, 1_030)).toEqual({
      warn: true,
      word: "did not start · relay: the engine is not signed in",
    });

    // **A BEAT THAT STOPPED HAPPENING TURNS OFF EVERY SCHEDULE**, and the
    // lateness is said first: it makes what the report says old news.
    const late = beatWords(broke, 1_000 + 5 * 60);
    expect(late?.warn).toBe(true);
    expect(late?.word).toBe("the beat stopped · last one 5m ago");
  });

  test("THE EMPTY BOARD SAYS WHERE IT LOOKED, with the real paths and what it found", () => {
    render(
      <BlankCanvas
        state="empty"
        brokenCount={0}
        onCreate={() => {}}
        places={{
          state: "ready",
          places: [
            { origin: "yours", path: "/home/x/.config/sailor/flows", exists: true, count: 0 },
            { origin: "this project", path: "/work/x/flows", exists: false, count: 0 },
          ],
        }}
      />,
    );
    const rows = Array.from(document.querySelectorAll(".blank__place"));
    expect(rows.map((row) => row.textContent)).toEqual([
      "yours/home/x/.config/sailor/flows0 found",
      "this project/work/x/flowsno such folder",
    ]);
    expect(rows[1].getAttribute("data-missing")).toBe("true");
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
      cache_write_long_tokens: null,
      cost_micros: 340_000,
      unmeasured: 0,
      unpriced: 0,
      tokens_by_model: {},
    };
    expect(spendWords(summary)).toBe("$0.34 today");
    expect(spendWords({ ...summary, unpriced: 3 })).toBe("at least $0.34 + ? today");
    expect(spendWords({ ...summary, cost_micros: 0, unpriced: 3 })).toBe("cost ? today");
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

  test("AN ENGINE THAT DOES NOT ANSWER IS SAID, never read as «nothing running»", async () => {
    // The absurd control: every command refused. The chip must not turn a
    // refusal into an empty list and call the machine quiet.
    const shell = pretendShell({});
    try {
      render(<LiveChip native now={1000} />);
      await screen.findByText(/cannot ask what runs: .*no open_runs/);
      expect(screen.queryByText(/nothing running/)).toBeNull();
    } finally {
      shell.stop();
    }
  });

  test("THE BAR LISTENS TO ONE CHANNEL, AND A BURST OF FACTS IS ONE ASK", async () => {
    // One piece of an engine's output is one fact here, hundreds a second: a
    // read on each turns a talkative step into a storm, and one of the reads
    // starts a command per profile.
    const shell = pretendShell({ open_runs: [], day_summary: { ledger_present: true, cost_micros: 0, unpriced: 0 } });
    const channels: string[] = [];
    const handlers: Array<(event: { payload: unknown }) => void> = [];
    (window as unknown as { __TAURI__: { event: unknown } }).__TAURI__.event = {
      listen: (channel: string, handler: (event: { payload: unknown }) => void) => {
        channels.push(channel);
        handlers.push(handler);
        return Promise.resolve(() => {});
      },
    };
    try {
      render(<LiveChip native now={1000} />);
      await screen.findByText("nothing running");
      await waitFor(() => expect(channels.length).toBeGreaterThan(0));
      expect(new Set(channels)).toEqual(new Set(["sailor_event"]));

      const asked = () => shell.calls.filter((call) => call.command === "open_runs").length;
      const before = asked();
      for (let piece = 0; piece < 50; piece += 1) {
        handlers.forEach((handler) => handler({ payload: { kind: "run", at: piece, payload: {} } }));
      }
      expect(asked(), "fifty facts asked fifty times").toBe(before);

      // **AND THE LAST FACT OF A BURST IS NOT LOST**: asked for at the end of
      // the second, or the chip keeps the state from before the run ended.
      await waitFor(() => expect(asked()).toBe(before + 1), { timeout: 3000 });
      expect(asked(), "one ask for the whole burst").toBe(before + 1);
    } finally {
      shell.stop();
    }
  });

  test("THE CHIP READS THE ENGINE, from any place: open_runs and day_summary are asked", async () => {
    const shell = pretendShell({ open_runs: [working], day_summary: { ledger_present: true, cost_micros: 340_000, unpriced: 0 } });
    try {
      render(<LiveChip native now={1000 + 60} />);
      await screen.findByText(/relay · 3 of 7 · at write-the-baton · 1m/);
      expect(screen.getByText("$0.34 today")).toBeTruthy();
      expect(shell.calls.map((call) => call.command)).toEqual(expect.arrayContaining(["open_runs", "day_summary"]));
    } finally {
      shell.stop();
    }
  });
});

describe("the ledger as a database", () => {
  const TABLES = {
    directory: "/home/mira/.config/sailor/ledger",
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

/**
 * **A SCREEN THAT LOSES ITS ROW GOES BACK INTO HIDING.** The seven were behind
 * one noun called «Sailor» and cost two clicks each; the day an eighth is added
 * to the list and not to the ground, it is invisible again and nothing says so.
 */
describe("the machine's ground and the screens it holds", () => {
  test("EVERY SCREEN THIS MACHINE HOLDS HAS A ROW, AND EVERY ROW A SCREEN", () => {
    // Both sides read from the files, so neither can drift from a list kept here.
    expect([...machineHolds()].sort()).toEqual([...tabsThatExist()].sort());
    expect(tabsThatExist().length).toBeGreaterThan(4);
  });

  test("and every row of that ground says what it answers, where it is offered", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /Search or run a command/ }));
    const hints = Array.from(document.querySelectorAll(".palette__hint")).map(
      (one) => one.textContent,
    );
    for (const row of MACHINE) {
      expect(hints, `«${row.name}» does not say what it answers`).toContain(row.asks);
    }
  });
});

/**
 * **THE WINDOW COMES BACK WHERE IT WAS.** A build of the engine replaces this
 * window; the terminals survive it, being held elsewhere, and until now
 * everything else did not — every build cost the walk back.
 */
describe("a window replaced by a build", () => {
  // Cleared on the way in as well: what an earlier test left behind is a place
  // this window would honestly reopen on, and the subject here is its own.
  beforeEach(() => window.localStorage.clear());
  afterEach(() => window.localStorage.clear());

  test("OPENS WHERE IT WAS LEFT, place and all", async () => {
    const first = render(<App />);
    await typeInThePalette(WHY);
    expect(
      first.container.querySelector(".body[hidden]"),
      "the board is still in view, so this proves nothing",
    ).not.toBeNull();
    cleanup();

    // A NEW WINDOW, not a re-render: this is what a swap leaves behind.
    const again = render(<App />);
    const crumbs = Array.from(again.container.querySelectorAll(".topbar__crumb")).map(
      (one) => one.textContent,
    );
    expect(crumbs[0], "it opened on the board again").toBe(WHY);
  });

  test("AND ON THE FLOW THAT WAS OPEN, not on the first of the list", () => {
    const first = render(<App />);
    const rows = Array.from(first.container.querySelectorAll<HTMLElement>("button.rail__item"));
    const other = rows.find((row) => row.getAttribute("data-open") === null) as HTMLElement;
    expect(other, "the column offers no second flow").toBeDefined();
    const name = other.querySelector(".rail__label")?.textContent ?? "";
    fireEvent.click(other);
    cleanup();

    const again = render(<App />);
    const open = again.container.querySelector("button.rail__item[data-open] .rail__label");
    expect(open?.textContent).toBe(name);
  });

  test("A FLOW THAT IS NOT THERE ANY MORE IS NOT A PLACE", () => {
    // Renamed, deleted, or belonging to a tree nobody stands in now: the board
    // opens where everybody starts instead of on a name nothing answers to.
    // Stamped now, or the note reads as old and the window opens on what waits
    // instead of on the board this test is about.
    window.localStorage.setItem(
      "sailor.where",
      JSON.stringify({ place: "board", focus: "a-ghost", at: Math.floor(Date.now() / 1000) }),
    );
    const { container } = render(<App />);
    const open = container.querySelectorAll("button.rail__item[data-open]");
    expect(open, "the board opened on nothing, or on two things").toHaveLength(1);
    expect(open[0].querySelector(".rail__label")?.textContent).not.toBe("a-ghost");
  });
});
