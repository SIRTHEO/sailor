// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import { Terminal as Emulator } from "@xterm/xterm";
import stylesheetSource from "./styles.css?raw";
import App from "./App";
import { belowThreshold, contrastPairs, parseStylesheet, type Stylesheet } from "./contrast";
import { ANOTHER_PATH, placesOf, Terminals, WORKSPACE_HINT } from "./Terminals";
import { movedLabel, routingNote, tokensLabel, whoLabel } from "./TerminalPane";
import { declaredCeiling } from "./terminal";
import {
  decodeBytes,
  encodeBytes,
  keyBytes,
  keyStroke,
  livenessOf,
  livenessWord,
  OutputBus,
  paneGesture,
  STILL_SPEAKING_MS,
  splitCommandLine,
  windowHold,
  type KeyAction,
  type TerminalSummary,
} from "./terminal";

/**
 * **WHAT THE REACT HALF OF THE TERMINAL CAN GET WRONG IN SILENCE.** An accent
 * lost in a long output, a key sent to routing instead of to the program, a
 * dead terminal drawn alive, a pane that comes back blank: none makes a noise.
 * The judge is the contract — the shell is faked from its document by hand,
 * while the components are the real ones, emulator included.
 */

afterEach(cleanup);

let sheet: Stylesheet;

beforeAll(() => {
  sheet = parseStylesheet(stylesheetSource);
  // xterm and React Flow ask the browser for things jsdom does not have. They
  // are scaffolding, not fakes over the code under test: a real window
  // supplies them.
  (window as unknown as { matchMedia: unknown }).matchMedia = () => ({
    matches: false,
    media: "",
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent: () => false,
  });
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

// ── bytes are not text ───────────────────────────────────────────────────

describe("the bytes going to the shell", () => {
  test("AN ACCENT LEAVES AS UTF-8, not in the latin-1 of `btoa`", () => {
    // `btoa("à")` neither fails nor warns: it answers the byte 0xE0, which in
    // the shell is a different letter. The defect only shows on accented words.
    const mine = encodeBytes(keyBytes("à"));
    expect(mine).toBe("w6A=");
    expect(mine).not.toBe(btoa("à"));
    expect(Array.from(decodeBytes(mine))).toEqual([0xc3, 0xa0]);
  });

  test("every byte from 0 to 255 comes back identical", () => {
    const every = new Uint8Array(256);
    for (let value = 0; value < 256; value += 1) every[value] = value;
    expect(Array.from(decodeBytes(encodeBytes(every)))).toEqual(Array.from(every));
  });
});

describe("the bytes coming out of the process", () => {
  test("A LETTER SPLIT ACROSS TWO EVENTS IS PUT BACK TOGETHER ON SCREEN", async () => {
    // This is why the contract says base64 and not string: `è` is 0xC3 0xA8,
    // and a pseudo-terminal may deliver them in two reads. The judge is the
    // real emulator's buffer, that is, what a person would read.
    const term = new Emulator({ cols: 20, rows: 4 });
    await write(term, decodeBytes(encodeBytes(new Uint8Array([0xc3]))));
    await write(term, decodeBytes(encodeBytes(new Uint8Array([0xa8, 0x21]))));
    expect(term.buffer.active.getLine(0)?.translateToString(true)).toBe("è!");
    term.dispose();
  });
});

function write(term: Emulator, bytes: Uint8Array): Promise<void> {
  return new Promise((done) => term.write(bytes, () => done()));
}

// ── what to start ────────────────────────────────────────────────────────

describe("what to start, split into a program and its arguments", () => {
  test("`claude --resume` IS `claude` WITH `--resume`, not a binary of that name", () => {
    expect(splitCommandLine("claude --resume")).toEqual({ program: "claude", args: ["--resume"] });
    expect(splitCommandLine("  codex  ")).toEqual({ program: "codex", args: [] });
    expect(splitCommandLine("")).toEqual({ program: undefined, args: [] });
  });

  test("quotes group words, as a shell would", () => {
    expect(splitCommandLine(`sh -c "echo hi there"`)).toEqual({ program: "sh", args: ["-c", "echo hi there"] });
    expect(splitCommandLine(`claude --title 'a long one'`)).toEqual({
      program: "claude",
      args: ["--title", "a long one"],
    });
  });
});

// ── submitting and pressing are two different roads ──────────────────────

function kinds(actions: KeyAction[]): string[] {
  return actions.map((action) => action.kind);
}

function submittedLines(actions: KeyAction[]): string[] {
  return actions.flatMap((action) => (action.kind === "submit" ? [action.line] : []));
}

describe("the two keys the window keeps for itself", () => {
  test("A TERMINAL YOU CANNOT COPY OUT OF IS NOT ONE YOU WORK IN", () => {
    expect(paneGesture({ key: "c", metaKey: true }, "meta")).toBe("copy");
    expect(paneGesture({ key: "v", metaKey: true }, "meta")).toBe("paste");
    expect(paneGesture({ key: "C", ctrlKey: true, shiftKey: true }, "ctrl_shift")).toBe("copy");
    expect(paneGesture({ key: "V", ctrlKey: true, shiftKey: true }, "ctrl_shift")).toBe("paste");
  });

  test("AND NOTHING TAKES CTRL-C, under either hold", () => {
    // Copy on the key that stops a runaway process leaves closing the pane as
    // the only way out of one.
    expect(paneGesture({ key: "c", ctrlKey: true }, "meta")).toBe("to_process");
    expect(paneGesture({ key: "c", ctrlKey: true }, "ctrl_shift")).toBe("to_process");
    expect(paneGesture({ key: "c", ctrlKey: true, metaKey: true }, "meta")).toBe("to_process");
  });

  test("every other key belongs to the process", () => {
    expect(paneGesture({ key: "c" }, "meta")).toBe("to_process");
    expect(paneGesture({ key: "v", ctrlKey: true }, "ctrl_shift")).toBe("to_process");
    expect(paneGesture({ key: "k", metaKey: true }, "meta")).toBe("to_process");
    expect(paneGesture({ key: "ArrowUp", metaKey: true }, "meta")).toBe("to_process");
    expect(paneGesture({ key: "c", metaKey: true, altKey: true }, "meta")).toBe("to_process");
  });

  test("the hold is the machine's, not a guess made once", () => {
    expect(windowHold("MacIntel")).toBe("meta");
    expect(windowHold("iPhone")).toBe("meta");
    expect(windowHold("Linux x86_64")).toBe("ctrl_shift");
    expect(windowHold("Win32")).toBe("ctrl_shift");
    expect(windowHold("")).toBe("ctrl_shift");
  });
});

describe("where a key goes", () => {
  test("NO KEY OTHER THAN ENTER ENDS UP IN ROUTING", () => {
    const keys = ["l", "s", " ", "-", "à", "\x7f", "\x03", "\x1b[A", "\x1b[B", "\t", "\x04", "\x15"];
    for (const key of keys) {
      for (const draft of ["", "cargo test"]) {
        for (const mode of ["compose", "raw"] as const) {
          const stroke = keyStroke(mode, draft, key);
          expect(submittedLines(stroke.actions), `«${key}» in ${mode} with «${draft}»`).toEqual([]);
        }
      }
    }
  });

  test("Enter delivers the whole line, and sends not one byte of it to the shell", () => {
    const stroke = keyStroke("compose", "cargo test -p terminal", "\r");
    expect(submittedLines(stroke.actions)).toEqual(["cargo test -p terminal"]);
    expect(kinds(stroke.actions)).not.toContain("press");
    expect(stroke.draft).toBe("");
  });

  test("composing a line sends nothing to the shell, one character at a time", () => {
    let draft = "";
    const sent: string[] = [];
    for (const key of "ls -la") {
      const stroke = keyStroke("compose", draft, key);
      draft = stroke.draft;
      sent.push(...kinds(stroke.actions));
    }
    expect(draft).toBe("ls -la");
    expect(sent).not.toContain("press");
    expect(sent).not.toContain("submit");
  });

  test("INSIDE AN EDITOR ENTER IS A KEY, not a line to route", () => {
    const stroke = keyStroke("raw", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
    expect(submittedLines(stroke.actions)).toEqual([]);
  });

  test("Ctrl-C always reaches whatever is running, even on a full line", () => {
    for (const mode of ["compose", "raw"] as const) {
      const stroke = keyStroke(mode, "a very long command", "\x03");
      expect(kinds(stroke.actions)).toContain("press");
      expect(stroke.draft).toBe("");
    }
  });

  test("an empty Enter is a newline, not a line to route", () => {
    const stroke = keyStroke("compose", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
  });

  test("on an empty line the terminal is a passthrough; on a full line it refuses", () => {
    expect(kinds(keyStroke("compose", "", "\x1b[A").actions)).toEqual(["press"]);
    const refused = keyStroke("compose", "ls", "\x1b[A");
    expect(kinds(refused.actions)).toEqual(["ignored"]);
    expect(refused.draft).toBe("ls");
  });

  test("backspace takes one character off the line and one off the screen", () => {
    const stroke = keyStroke("compose", "lsx", "\x7f");
    expect(stroke.draft).toBe("ls");
    expect(stroke.actions).toEqual([{ kind: "echo", text: "\b \b" }]);
  });
});

describe("where the line ended up, told to whoever is watching", () => {
  test("a rerouted line names the rule, the flow, and whether the flow started", () => {
    const sent = {
      kind: "flow" as const,
      flow: "dispatch-the-work",
      text: "find the leftovers",
      rule: "marked-request",
    };
    const started = routingNote("? find the leftovers", { ...sent, run_id: "dispatch-the-work-9" });
    expect(started).toContain("marked-request");
    expect(started).toContain("dispatch-the-work");
    expect(started).toContain("run dispatch-the-work-9 started");

    // The flow that refused says why, in the engine's words; and an engine
    // that started nothing and said nothing is reported as exactly that.
    expect(routingNote("? x", { ...sent, refused: "no flow is called dispatch-the-work" })).toContain(
      "did not start: no flow is called dispatch-the-work",
    );
    expect(routingNote("? x", sent)).toContain("nothing ran it");
  });

  test("a command says it went to the shell", () => {
    expect(routingNote("ls", { kind: "command" })).toContain("shell");
  });

  test("the bytes moved are said in a unit a person reads, and never rounded to nothing", () => {
    expect(movedLabel(0)).toBe("0 bytes moved");
    expect(movedLabel(2048)).toBe("2 KB moved");
    expect(movedLabel(3 * 1024 * 1024)).toBe("3.0 MB moved");
    // The estimate is marked as one, and the ceiling appears only when a flow
    // declares it: a made-up budget would be a decision nobody took.
    expect(tokensLabel(840, null)).toBe("≈ 840 tokens");
    expect(tokensLabel(61521, null)).toBe("≈ 62k tokens");
    expect(tokensLabel(61521, 500000)).toBe("≈ 62k of 500k tokens");
    expect(tokensLabel(1_250_000, 500000)).toBe("≈ 1.3M of 500k tokens");
    // The ceiling is read from the measuring step's own `with`, nowhere else.
    const measuring = { id: "m", deps: [], action: "measure_terminal", when: null, max_attempts: 1, input_schema: { type: "any" as const }, output_schema: { type: "any" as const } };
    expect(declaredCeiling([{ ...measuring, with: { tty: "ttys004", ceiling: 500000 } }])).toBe(500000);
    expect(declaredCeiling([{ ...measuring, with: { tty: "ttys004" } }])).toBeNull();
    expect(declaredCeiling([{ ...measuring, action: "shell_check", with: { ceiling: 500000 } }])).toBeNull();
    expect(declaredCeiling([])).toBeNull();
  });
});

// ── alive, closed, or no longer known ────────────────────────────────────

function summary(over: Partial<TerminalSummary>): TerminalSummary {
  return {
    id: "t1",
    workspaceRoot: "/work/sailor",
    workspaceName: "sailor",
    alive: true,
    processId: 4242,
    device: "ttys004",
    moved: 0,
    estimatedTokens: 60129,
    program: "zsh",
    profile: null,
    ...over,
  };
}

describe("who runs in the pane", () => {
  test("the program, the profile when one applies, and an older host said as such", () => {
    expect(whoLabel({ program: "zsh", profile: null })).toBe("zsh");
    expect(whoLabel({ program: "codex", profile: "prove" })).toBe("codex · as prove");
    // The control: no program is not a shell, it is a host that did not say.
    expect(whoLabel({ program: "", profile: null })).toBe("program not reported by this host");
  });
});

describe("how a terminal is doing", () => {
  test("the event wins over the list, which is one round of polling old", () => {
    const closed = new Map([["t1", "exited with 0"]]);
    expect(livenessOf(summary({ alive: true }), closed, true)).toEqual({
      state: "closed",
      status: "exited with 0",
    });
  });

  test("a list that calls it closed does not invent an exit status", () => {
    expect(livenessOf(summary({ alive: false }), new Map(), true)).toEqual({
      state: "closed",
      status: null,
    });
  });

  test("WITHOUT THE EVENT CHANNEL WE DO NOT SAY «alive»: we say «no longer known»", () => {
    const mine = livenessOf(summary({ alive: true }), new Map(), false);
    expect(mine.state).toBe("unknown");
    expect(mine.state === "unknown" && mine.why).toContain("would not know");
  });

  test("with the channel attached and the list calling it alive, it is alive", () => {
    expect(livenessOf(summary({}), new Map(), true)).toEqual({ state: "alive" });
  });

  test("the three states have three different words: colour alone does not carry state", () => {
    const words = [
      livenessWord({ state: "alive" }),
      livenessWord({ state: "closed", status: null }),
      livenessWord({ state: "unknown", why: "x" }),
    ];
    expect(new Set(words).size).toBe(3);
  });
});

/**
 * **«ALIVE» IS NOT «TALKING».** One that has just printed thirty lines and one
 * stuck for eight minutes were drawn the same way. The answer is taken where
 * every byte already passes, since the bytes stay out of React.
 */
describe("which terminals are speaking", () => {
  const clock = { at: 1_000 };
  const busWithClock = () => new OutputBus(() => clock.at);

  test("A TERMINAL THAT SPEAKS IS IN THE SET, and stops being after the silence", () => {
    vi.useFakeTimers();
    clock.at = 1_000;
    const bus = busWithClock();
    const seen: string[][] = [];
    bus.watchSpeaking((now) => seen.push([...now]));

    bus.deliver("t1", new Uint8Array([1]), 0);
    expect([...bus.speaking()]).toEqual(["t1"]);
    expect(seen, "nobody was told it started").toEqual([["t1"]]);

    // A ring that stopped between two lines of a build would flicker at each.
    clock.at += STILL_SPEAKING_MS / 2;
    vi.advanceTimersByTime(STILL_SPEAKING_MS / 2);
    expect([...bus.speaking()]).toEqual(["t1"]);

    clock.at += STILL_SPEAKING_MS;
    vi.advanceTimersByTime(STILL_SPEAKING_MS);
    expect([...bus.speaking()]).toEqual([]);
    expect(seen[seen.length - 1], "nobody was told it stopped").toEqual([]);
    vi.useRealTimers();
  });

  test("SPEECH IS COUNTED WITH OR WITHOUT A PANE, and per terminal", () => {
    vi.useFakeTimers();
    clock.at = 1_000;
    const bus = busWithClock();

    // Nobody subscribed for `t2`: the bytes are lost, the fact that it talks
    // is not — the tab says so before a pane exists.
    expect(bus.deliver("t2", new Uint8Array([1]), 0)).toBe(false);
    bus.deliver("t1", new Uint8Array([1]), 0);
    expect([...bus.speaking()].sort()).toEqual(["t1", "t2"]);

    // Only one keeps talking.
    clock.at += STILL_SPEAKING_MS;
    bus.deliver("t1", new Uint8Array([2]), 0);
    vi.advanceTimersByTime(STILL_SPEAKING_MS);
    expect([...bus.speaking()]).toEqual(["t1"]);
    vi.useRealTimers();
  });

  test("THE CLOCK RUNS ONLY WHILE SOMEBODY TALKS: a window open for weeks", () => {
    vi.useFakeTimers();
    clock.at = 1_000;
    const bus = busWithClock();
    expect(vi.getTimerCount(), "a timer before a single byte").toBe(0);

    bus.deliver("t1", new Uint8Array([1]), 0);
    expect(vi.getTimerCount(), "nobody is watching the silence").toBe(1);

    clock.at += STILL_SPEAKING_MS * 2;
    vi.advanceTimersByTime(STILL_SPEAKING_MS * 2);
    expect(vi.getTimerCount(), "the timer ticks on through the quiet night").toBe(0);
    vi.useRealTimers();
  });

  test("THE WORD SAYS IT TOO, for whoever refuses the motion", () => {
    expect(livenessWord({ state: "alive" }, true)).toBe("speaking");
    expect(livenessWord({ state: "alive" }, false)).toBe("alive");
    // Speech is something an alive one does: an ended terminal does not
    // acquire a fourth word by having bytes in flight.
    expect(livenessWord({ state: "closed", status: null }, true)).toBe("ended");
  });
});

describe("output reaches the right pane", () => {
  test("bytes go to whoever subscribed for that id, and to nobody else", () => {
    const bus = new OutputBus();
    const mine: number[] = [];
    bus.subscribe("t1", (bytes) => mine.push(...bytes));
    expect(bus.deliver("t1", new Uint8Array([1, 2]), 0)).toBe(true);
    // BYTES FOR A TERMINAL WITH NO PANE ARE LOST BYTES, and whoever loses them
    // in silence shows an empty screen where there was output.
    expect(bus.deliver("t2", new Uint8Array([9]), 0)).toBe(false);
    expect(mine).toEqual([1, 2]);
  });
});

// ── the places a terminal opens in ───────────────────────────────────────

describe("the places offered", () => {
  test("THE OFFERED PLACES ARE THE ENGINE'S ANSWER, projects and worktrees, one entry per root", () => {
    const places = placesOf(
      [
        { root: "/work/sailor", name: "sailor", first_seen: 1, last_seen: 2, standing: "declared", current: true },
        { root: "/work/other", name: "other", first_seen: 1, last_seen: 2, standing: "gone", current: false },
      ],
      [
        { name: "sailor", path: "/work/sailor", branch: "main", locked: false, prunable: false, current: true },
        { name: "x", path: "/work/sailor-worktrees/x", branch: "work/x", locked: false, prunable: false, current: false },
      ],
    );
    expect(places.map((place) => place.root)).toEqual(["/work/sailor", "/work/other", "/work/sailor-worktrees/x"]);
    expect(places[2].label).toContain("work/x");
    // The absurd control: nothing known, nothing offered — the list is not
    // seeded from anywhere in the screen.
    expect(placesOf([], [])).toEqual([]);
  });
});

// ── on screen ────────────────────────────────────────────────────────────

interface Call {
  command: string;
  args: Record<string, unknown> | undefined;
}

interface FakeShell {
  /** Delivers an event the way the bridge would send it. */
  emit: (event: string, payload: unknown) => void;
  /** Which commands were asked, in order, with what. */
  calls: Call[];
  /** Just the names, for assertions that do not look at the arguments. */
  asked: string[];
  /** The arguments of the given `command` alone, in order. */
  argsOf: (command: string) => Array<Record<string, unknown> | undefined>;
  stop: () => void;
}

const EMPTY_BACKLOG = { at: 0, bytes: "", upto: 0, ended: null };

/**
 * Fakes the shell: the seven commands and the two events of the contract.
 * **`listen` IS EITHER THERE OR NOT, AND IT IS A PARAMETER**: without the
 * channel the screen must say «no longer known», and a rule that cannot be
 * removed cannot be tested. A pane always asks for its backlog.
 */
function pretendShell(answers: Record<string, unknown>, withEvents = true): FakeShell {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  const handlers = new Map<string, Array<(event: { payload: unknown }) => void>>();
  const calls: Call[] = [];
  const asked: string[] = [];
  const answered: Record<string, unknown> = { terminal_backlog: EMPTY_BACKLOG, ...answers };
  const shell: Record<string, unknown> = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        asked.push(command);
        if (!(command in answered)) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(answered[command]);
      },
    },
  };
  if (withEvents) {
    shell.event = {
      listen: (event: string, handler: (event: { payload: unknown }) => void) => {
        const list = handlers.get(event) ?? [];
        list.push(handler);
        handlers.set(event, list);
        return Promise.resolve(() => {
          handlers.set(event, (handlers.get(event) ?? []).filter((one) => one !== handler));
        });
      },
    };
  }
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = shell;
  return {
    calls,
    asked,
    argsOf: (command) => calls.filter((call) => call.command === command).map((call) => call.args),
    emit: (event, payload) => {
      for (const handler of handlers.get(event) ?? []) handler({ payload });
    },
    stop: () => {
      (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
    },
  };
}

const TWO: TerminalSummary[] = [
  { id: "t1", workspaceRoot: "/work/sailor", workspaceName: "sailor", alive: true, processId: 4242, device: "ttys004", moved: 2048, estimatedTokens: 61521, program: "codex", profile: "prove" },
  { id: "t2", workspaceRoot: "/work/other-repo/packages", workspaceName: "packages", alive: true, processId: 4243, device: "ttys009", moved: 0, estimatedTokens: 60129, program: "zsh", profile: null },
];

const PLACES = {
  workspaces: [{ root: "/work/sailor", name: "sailor", first_seen: 1, last_seen: 2, standing: "declared", current: true }],
  worktree_list: [{ name: "x", path: "/work/sailor-worktrees/x", branch: "work/x", locked: false, prunable: false, current: false }],
};

/**
 * The drawn DOM, measured whole: `:root` carries the roles.
 * **WHOEVER MEASURES MUST BE MEASURED**: the scene declares how many pairs it
 * expects to have found, because a check that looks at nothing passes.
 */
function measure(atLeast: number): string[] {
  const pairs = contrastPairs(document.documentElement, sheet);
  expect(pairs.length).toBeGreaterThanOrEqual(atLeast);
  return belowThreshold(pairs);
}

describe("the terminals screen", () => {
  test("EVERY TERMINAL IS ON SCREEN AT ONCE, the focused one first, and the line under it goes to routing", async () => {
    const shell = pretendShell({ terminal_list: TWO, terminal_submit: { kind: "command" } });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys009/ });
      const shown = () => Array.from(document.querySelectorAll(".pane:not([hidden])"));
      expect(shown()).toHaveLength(2);
      expect(shown()[0].getAttribute("data-focus")).toBe("true");
      expect(shown()[0].querySelector(".pane__device")?.textContent).toBe("ttys004");
      // The estimate travels in the row and is read on the pane, as an estimate.
      expect(shown()[0].querySelector(".pane__tokens")?.textContent).toBe("≈ 62k tokens");

      // Pressing the other tab brings that pane first and large; the first
      // pane does not disappear, it moves beside.
      fireEvent.click(screen.getByRole("button", { name: /ttys009/ }));
      expect(shown()).toHaveLength(2);
      expect(shown()[0].querySelector(".pane__device")?.textContent).toBe("ttys009");
      expect(shown()[1].getAttribute("data-focus")).toBeNull();

      // The line under the focused pane is confirmed with Enter and reaches
      // the router for that terminal, then the box is empty again.
      const line = screen.getByLabelText("a line for ttys009") as HTMLInputElement;
      fireEvent.change(line, { target: { value: "git status" } });
      await act(async () => {
        fireEvent.submit(line.closest("form") as HTMLFormElement);
      });
      const sent = shell.calls.find((call) => call.command === "terminal_submit");
      expect(sent?.args).toMatchObject({ line: "git status" });
      expect(line.value).toBe("");
    } finally {
      shell.stop();
    }
  });

  test("the list comes from `terminal_list`, and every tab carries its tty and its word", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(shell.asked).toContain("terminal_list");
      expect(screen.getAllByText("alive").length).toBeGreaterThan(0);
      // A TAB SAYS WHICH SESSION IT IS: the tty, not a guessed title.
      expect(screen.getByRole("button", { name: /ttys004/ })).toBeTruthy();
      expect(screen.getByRole("button", { name: /ttys009/ })).toBeTruthy();
      // And the pane on screen says it too, with what it has moved so far.
      expect(document.querySelector(".pane:not([hidden]) .pane__device")?.textContent).toBe("ttys004");
      expect(document.querySelector(".pane:not([hidden]) .pane__moved")?.textContent).toBe("2 KB moved");
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("A DEAD TERMINAL LOOKS DEAD: `terminal_closed` changes the state shown", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(screen.queryByText("ended")).toBeNull();

      await act(async () => {
        shell.emit("terminal_closed", { id: "t1", status: "exited with 130" });
      });

      expect(screen.getAllByText("ended").length).toBeGreaterThan(0);
      expect(screen.getByText("exited with 130")).toBeTruthy();
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("WITHOUT THE EVENT CHANNEL the screen declares nobody alive", async () => {
    const shell = pretendShell({ terminal_list: TWO }, false);
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(screen.queryByText("alive")).toBeNull();
      expect(screen.getAllByText("no longer known").length).toBeGreaterThan(0);
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("A TERMINAL IS OPENED BY CHOOSING A WORKSPACE the engine knows, and the choice reaches the command", async () => {
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0], ...PLACES });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const choice = (await screen.findByRole("combobox")) as HTMLSelectElement;
      await waitFor(() => {
        expect(Array.from(choice.options).map((option) => option.value)).toEqual([
          "/work/sailor",
          "/work/sailor-worktrees/x",
          ANOTHER_PATH,
        ]);
      });
      // The worktree is offered by the engine, and picking it is enough.
      fireEvent.change(choice, { target: { value: "/work/sailor-worktrees/x" } });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Open a terminal" }));
      });
      expect(shell.argsOf("terminal_open")).toEqual([
        { workspaceRoot: "/work/sailor-worktrees/x", program: undefined, args: undefined, cols: 80, rows: 24 },
      ]);
    } finally {
      shell.stop();
    }
  });

  test("A SECOND TERMINAL IS ONE GESTURE: the form stops asking what it knows", async () => {
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_open: TWO[1], ...PLACES });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      // With one open, the answers are known — the tree you stand in, the shell
      // you used — and a form asking them again asks you to confirm them.
      await waitFor(() => expect(screen.getByRole("button", { name: "New terminal" })).toBeTruthy());
      expect(screen.queryByRole("combobox"), "the form still asks where").toBeNull();

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
      });
      expect(shell.argsOf("terminal_open")).toHaveLength(1);

      // And the way to somewhere else is one press away, not gone.
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /somewhere else/ }));
      });
      expect(screen.queryByRole("combobox"), "there is no way to choose another tree").not.toBeNull();
    } finally {
      shell.stop();
    }
  });

  test("THE TREE YOU ARE STANDING IN IS THE ONE IT OPENS IN, not the head of a list", async () => {
    // The one being worked in is second here on purpose: taking the first would
    // pass by accident.
    const shell = pretendShell({
      terminal_list: [],
      terminal_open: TWO[0],
      workspaces: [
        { root: "/work/altro", name: "altro", first_seen: 1, last_seen: 9, standing: "declared", current: false },
        { root: "/work/qui", name: "qui", first_seen: 1, last_seen: 2, standing: "declared", current: true },
      ],
      worktree_list: [],
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await waitFor(() => expect(screen.getByRole("combobox")).toBeTruthy());
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Open a terminal" }));
      });
      expect(shell.argsOf("terminal_open")[0]).toMatchObject({ workspaceRoot: "/work/qui" });
    } finally {
      shell.stop();
    }
  });

  test("with nothing known, only a typed path is offered, and nothing opens without one", async () => {
    // THE ABSURD CONTROL of the test above: an engine with no projects and no
    // worktrees leaves the choice empty. A screen that kept its own list
    // would still offer something here.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const choice = (await screen.findByRole("combobox")) as HTMLSelectElement;
      expect(Array.from(choice.options).map((option) => option.value)).toEqual([ANOTHER_PATH]);
      const button = screen.getByRole("button", { name: "Open a terminal" }) as HTMLButtonElement;
      expect(button.disabled).toBe(true);

      fireEvent.change(screen.getByPlaceholderText(WORKSPACE_HINT), { target: { value: "/work/other-repo" } });
      expect((screen.getByRole("button", { name: "Open a terminal" }) as HTMLButtonElement).disabled).toBe(false);
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Open a terminal" }));
      });
      expect(shell.argsOf("terminal_open")).toEqual([
        { workspaceRoot: "/work/other-repo", program: undefined, args: undefined, cols: 80, rows: 24 },
      ]);
    } finally {
      shell.stop();
    }
  });

  test("THE ENGINES ARE OFFERED BY NAME, AND THE NAMES ARE NOT THIS SCREEN'S", async () => {
    // The list is the table of command lines: a machine carrying other engines
    // offers those, and nothing here has to be edited for it to happen.
    const shell = pretendShell({
      terminal_list: [],
      terminal_open: TWO[0],
      ...PLACES,
      profile_command_lines: [
        { id: "un-motore", display_name: "Un Motore", executable: "unmotore" },
        { id: "un-altro", display_name: "Un Altro", executable: "unaltro" },
      ],
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const chosen = await screen.findByRole("button", { name: "Un Altro" });
      await act(async () => {
        fireEvent.click(chosen);
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Open a terminal" }));
      });
      expect(shell.argsOf("terminal_open")[0]).toMatchObject({ program: "unaltro", args: undefined });
    } finally {
      shell.stop();
    }
  });

  test("WHAT TO START CROSSES THE BRIDGE AS A PROGRAM AND ITS ARGUMENTS", async () => {
    // Typing `claude --resume` must not look for a binary named, literally,
    // `claude --resume`: the field is split before the call.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0], ...PLACES });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("combobox");
      fireEvent.change(screen.getByPlaceholderText(/a command line with its options/), { target: { value: "claude --resume" } });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Open a terminal" }));
      });
      expect(shell.argsOf("terminal_open")).toEqual([
        { workspaceRoot: "/work/sailor", program: "claude", args: ["--resume"], cols: 80, rows: 24 },
      ]);
    } finally {
      shell.stop();
    }
  });

  test("an empty list is not confused with an engine that does not answer", async () => {
    const shell = pretendShell({ terminal_list: [] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByText(/No terminal is open/);
      cleanup();
      render(
        <div className="app">
          <Terminals native={false} />
        </div>,
      );
      expect(screen.getByText(/I cannot ask/)).toBeTruthy();
    } finally {
      shell.stop();
    }
  });
});

// ── the wiring, not the pieces ───────────────────────────────────────────

/** The on-screen pane's keyboard: xterm listens on a hidden textarea. */
function keyboardOf(): HTMLTextAreaElement {
  const area = document.querySelector(".pane:not([hidden]) .xterm-helper-textarea");
  expect(area, "the on-screen pane has no textarea: the emulator did not mount").toBeTruthy();
  return area as HTMLTextAreaElement;
}

/** What can be read on the on-screen pane's screen. */
function paneScreenText(): string {
  return document.querySelector(".pane:not([hidden]) .xterm-rows")?.textContent ?? "";
}

/** A key that is not a letter: xterm derives it from `keydown`. */
function tapKey(area: HTMLElement, init: KeyboardEventInit): void {
  area.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
}

/** A letter: xterm derives it from `keypress`. */
function typeLetter(area: HTMLElement, letter: string): void {
  const code = letter.charCodeAt(0);
  area.dispatchEvent(
    new KeyboardEvent("keypress", {
      key: letter,
      keyCode: code,
      charCode: code,
      bubbles: true,
      cancelable: true,
    }),
  );
}

function occurrences(text: string, needle: string): number {
  return text.split(needle).length - 1;
}

describe("the wiring between the bridge and the screen", () => {
  test("OUTPUT REACHES THE PANE'S EMULATOR, and a split letter is put back together there", async () => {
    const shell = pretendShell({ terminal_list: [TWO[0]] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      // The backlog has been asked for and answered empty before any event.
      await waitFor(() => expect(shell.asked).toContain("terminal_backlog"));

      await act(async () => {
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xc3])), at: 0 });
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xa8, 0x21])), at: 1 });
      });

      await waitFor(() => {
        expect(paneScreenText()).toContain("è!");
      });
    } finally {
      shell.stop();
    }
  });

  test("A PANE ATTACHED LATE SHOWS THE BACKLOG FIRST, then only what follows it", async () => {
    // The scene: the terminal printed «before-42» while nobody looked, and
    // the bridge keeps sending events. A piece the backlog already holds
    // arrives too (the bridge attached before the pane did): it must not be
    // shown twice. A piece past the backlog must be shown once.
    const backlog = { at: 0, bytes: encodeBytes(keyBytes("before-42\r\n")), upto: 11, ended: null };
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_backlog: backlog });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      await act(async () => {
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(keyBytes("before-42\r\n")), at: 0 });
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(keyBytes("after-48\r\n")), at: 11 });
      });
      await waitFor(() => {
        expect(paneScreenText()).toContain("after-48");
      });
      expect(occurrences(paneScreenText(), "before-42")).toBe(1);
      expect(occurrences(paneScreenText(), "after-48")).toBe(1);
    } finally {
      shell.stop();
    }
  });

  test("LEAVING THE SCREEN AND COMING BACK SHOWS WHAT WAS PRINTED MEANWHILE", async () => {
    // The screen is mounted, unmounted and mounted again. The second mount
    // finds the pane blank — the emulator was destroyed with it — and asks
    // the engine what it missed: the backlog is on the pane, once.
    const backlog = { at: 0, bytes: encodeBytes(keyBytes("printed-while-away\r\n")), upto: 20, ended: null };
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_backlog: backlog });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      await waitFor(() => expect(paneScreenText()).toContain("printed-while-away"));
      const askedBefore = shell.argsOf("terminal_backlog").length;
      cleanup();
      expect(document.querySelector(".pane")).toBeNull();

      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      await waitFor(() => expect(paneScreenText()).toContain("printed-while-away"));
      expect(shell.argsOf("terminal_backlog").length).toBeGreaterThan(askedBefore);
      expect(occurrences(paneScreenText(), "printed-while-away")).toBe(1);
    } finally {
      shell.stop();
    }
  });

  test("A KEY GOES TO `terminal_press`, ENTER TO `terminal_submit`, AND NEVER BOTH", async () => {
    const shell = pretendShell({
      terminal_list: [TWO[0]],
      terminal_press: null,
      terminal_submit: { kind: "command" },
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      const keys = keyboardOf();

      // HOW IT IS BORN: A TERMINAL IS BORN A TERMINAL, and the letter is what
      // proves it: only the `x` tells the two modes apart.
      expect(screen.getByText("keys go straight to the process")).toBeTruthy();
      await act(async () => {
        typeLetter(keys, "x");
        tapKey(keys, { key: "ArrowUp", keyCode: 38 });
        tapKey(keys, { key: "c", keyCode: 67, ctrlKey: true });
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_press").map((args) => args?.bytes)).toEqual([
        encodeBytes(keyBytes("x")),
        encodeBytes(keyBytes("\x1b[A")),
        encodeBytes(keyBytes("\x03")),
        encodeBytes(keyBytes("\r")),
      ]);
      expect(shell.asked).not.toContain("terminal_submit");

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "compose a line to route" }));
      });

      // Composing sends nothing to anybody: if even one character left,
      // `terminal_submit` — which writes the line itself — would run it twice.
      await act(async () => {
        typeLetter(keys, "l");
        typeLetter(keys, "s");
      });
      expect(shell.argsOf("terminal_press").length).toBe(4);
      expect(shell.asked).not.toContain("terminal_submit");

      await act(async () => {
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "ls" }]);
      expect(shell.argsOf("terminal_press").length).toBe(4);

      await waitFor(() => {
        expect(screen.getByText(/went to the shell/)).toBeTruthy();
      });
    } finally {
      shell.stop();
    }
  });

  test("a rerouted line never reaches the shell, and says so with the rule", async () => {
    const shell = pretendShell({
      terminal_list: [TWO[0]],
      terminal_press: null,
      terminal_submit: {
        kind: "flow",
        flow: "dispatch-the-work",
        text: "find the leftovers",
        rule: "marked-request",
      },
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      const keys = keyboardOf();
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "compose a line to route" }));
      });
      await act(async () => {
        typeLetter(keys, "?");
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "?" }]);
      expect(shell.asked).not.toContain("terminal_press");
      await waitFor(() => {
        expect(screen.getByText(/marked-request/)).toBeTruthy();
      });
    } finally {
      shell.stop();
    }
  });

  // `terminal_resize` HAS NO TEST, AND HERE IS WHY. The wiring starts from a
  // `ResizeObserver` and from `fit()`, which measure real frames: in jsdom
  // every frame is zero by zero, `fit()` produces no new size and `refit`
  // returns without calling anything. It is verified by opening the window.

  test("closing a terminal asks the engine, with its id", async () => {
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_close: null });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /ttys004/ });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Close this terminal" }));
      });
      expect(shell.argsOf("terminal_close")).toEqual([{ id: "t1" }]);
    } finally {
      shell.stop();
    }
  });
});

// ── the screen inside the window ─────────────────────────────────────────

describe("the terminals inside the window", () => {
  test("THE SCREEN STAYS MOUNTED BEHIND THE OTHER PLACES: going to Flows and back destroys no pane", () => {
    // Outside the shell the screen is mute, and that is enough: what is
    // measured is that the element survives the change of place, hidden.
    const { container } = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /^Terminals/ }));
    const terminals = container.querySelector(".terminals");
    expect(terminals, "the terminals screen did not draw").toBeTruthy();
    expect((terminals as HTMLElement).hidden).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: /^Board/ }));
    const behind = container.querySelector(".terminals");
    expect(behind, "the terminals screen was unmounted on leaving it").toBeTruthy();
    expect(behind).toBe(terminals);
    expect((behind as HTMLElement).hidden).toBe(true);
  });
});
