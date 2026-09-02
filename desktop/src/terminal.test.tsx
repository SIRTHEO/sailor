// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { Terminal as Emulator } from "@xterm/xterm";
import stylesheetSource from "./styles.css?raw";
import { belowThreshold, contrastPairs, parseStylesheet, type Stylesheet } from "./contrast";
import { Terminals, WORKSPACE_HINT } from "./Terminals";
import { routingNote } from "./TerminalPane";
import {
  decodeBytes,
  encodeBytes,
  keyBytes,
  keyStroke,
  livenessOf,
  livenessWord,
  OutputBus,
  type KeyAction,
  type TerminalSummary,
} from "./terminal";

/**
 * **THE FOUR THINGS THE REACT HALF OF THE TERMINAL CAN GET WRONG IN SILENCE.**
 * An accent lost inside a long output, a key sent to routing instead of to the
 * program, a dead terminal drawn alive, a contrast pair below threshold: none
 * makes a noise, the window draws the same, and whoever looks believes it. The
 * judge is the contract, not the bridge — the shell is faked from
 * `docs/2026-09-01-il-contratto-del-terminale.md` by hand, while the components
 * are the real ones, emulator included.
 */

afterEach(cleanup);

let sheet: Stylesheet;

beforeAll(() => {
  sheet = parseStylesheet(stylesheetSource);
  // xterm asks the browser for two things jsdom does not have. They are
  // scaffolding, not fakes over the code under test: a real window supplies
  // them.
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
});

// ── bytes are not text ───────────────────────────────────────────────────

describe("the bytes going to the shell", () => {
  test("AN ACCENT LEAVES AS UTF-8, not in the latin-1 of `btoa`", () => {
    // `btoa("à")` neither fails nor warns: it answers the byte 0xE0, which in
    // the shell is a different letter. The defect only shows on accented words,
    // that is, on the ones written here all day long.
    const mine = encodeBytes(keyBytes("à"));
    expect(mine).toBe("w6A=");
    expect(mine).not.toBe(btoa("à"));
    expect(Array.from(decodeBytes(mine))).toEqual([0xc3, 0xa0]);
  });

  test("every byte from 0 to 255 comes back identical", () => {
    // The newline, the escape, the Ctrl-C and the high bytes of a multibyte
    // character all pass through here.
    const every = new Uint8Array(256);
    for (let value = 0; value < 256; value += 1) every[value] = value;
    expect(Array.from(decodeBytes(encodeBytes(every)))).toEqual(Array.from(every));
  });
});

describe("the bytes coming out of the process", () => {
  test("A LETTER SPLIT ACROSS TWO EVENTS IS PUT BACK TOGETHER ON SCREEN", async () => {
    // This is why the contract says base64 and not string. The two events are
    // the ones the bridge will send — `è` is 0xC3 0xA8, and a pseudo-terminal
    // may deliver them in two reads — and the judge is the real emulator's
    // buffer, that is, what a person would read.
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

// ── submitting and pressing are two different roads ──────────────────────

function kinds(actions: KeyAction[]): string[] {
  return actions.map((action) => action.kind);
}

function submittedLines(actions: KeyAction[]): string[] {
  return actions.flatMap((action) => (action.kind === "submit" ? [action.line] : []));
}

describe("where a key goes", () => {
  test("NO KEY OTHER THAN ENTER ENDS UP IN ROUTING", () => {
    // The defect it guards against: sending everything to `submit` would have
    // every arrow and every Ctrl-C examined by a set of rules that has nothing
    // to do with them, and an editor inside the terminal would be unusable.
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
    // If even a single character had already left, `terminal_submit` — which
    // writes the line itself — would have it run twice: «lsls».
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
    // This is the case the contract names in full, and it is the whole reason
    // `submit` and `press` are two commands and not one.
    const stroke = keyStroke("raw", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
    expect(submittedLines(stroke.actions)).toEqual([]);
  });

  test("Ctrl-C always reaches whatever is running, even on a full line", () => {
    // A way to stop what is running is taken from nobody, in no way: it is the
    // one exception to «while composing, nothing leaves».
    for (const mode of ["compose", "raw"] as const) {
      const stroke = keyStroke(mode, "un comando lunghissimo", "\x03");
      expect(kinds(stroke.actions)).toContain("press");
      expect(stroke.draft).toBe("");
    }
  });

  test("an empty Enter is a newline, not a line to route", () => {
    const stroke = keyStroke("compose", "", "\r");
    expect(kinds(stroke.actions)).toEqual(["press"]);
  });

  test("on an empty line the terminal is a passthrough; on a full line it refuses", () => {
    // Arrow up on an empty line brings back the previous command from the shell.
    expect(kinds(keyStroke("compose", "", "\x1b[A").actions)).toEqual(["press"]);
    // On a full line it does not leave, and says why: leaving would desync the
    // screen, because that line is held by the window and not by the shell's
    // `readline`.
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
  test("a rerouted line names the rule and the flow, not just the flow", () => {
    // Without the name of the rule, whoever is watching knows the line was not
    // run and has no way back to the line of JSON that decided it.
    const said = routingNote("? trova i residui", {
      kind: "flow",
      flow: "dispatch-the-work",
      text: "trova i residui",
      rule: "marked-request",
    });
    expect(said).toContain("marked-request");
    expect(said).toContain("dispatch-the-work");
    expect(said).toContain("non è stata eseguita");
  });

  test("a command says it went to the shell", () => {
    expect(routingNote("ls", { kind: "command" })).toContain("shell");
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
    ...over,
  };
}

describe("how a terminal is doing", () => {
  test("the event wins over the list, which is one round of polling old", () => {
    const closed = new Map([["t1", "uscita 0"]]);
    expect(livenessOf(summary({ alive: true }), closed, true)).toEqual({
      state: "closed",
      status: "uscita 0",
    });
  });

  test("a list that calls it closed does not invent an exit status", () => {
    expect(livenessOf(summary({ alive: false }), new Map(), true)).toEqual({
      state: "closed",
      status: null,
    });
  });

  test("WITHOUT THE EVENT CHANNEL WE DO NOT SAY «vivo»: we say «non lo so più»", () => {
    // If `terminal_closed` cannot arrive, «vivo» is a claim this screen cannot
    // make, and a pane that makes it makes it forever — death will never come.
    const mine = livenessOf(summary({ alive: true }), new Map(), false);
    expect(mine.state).toBe("unknown");
    expect(mine.state === "unknown" && mine.why).toContain("non lo saprebbe");
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

describe("output reaches the right pane", () => {
  test("bytes go to whoever subscribed for that id, and to nobody else", () => {
    const bus = new OutputBus();
    const mine: number[] = [];
    bus.subscribe("t1", (bytes) => mine.push(...bytes));
    expect(bus.deliver("t1", new Uint8Array([1, 2]))).toBe(true);
    // BYTES FOR A TERMINAL WITH NO PANE ARE LOST BYTES, and whoever loses them
    // in silence shows an empty screen where there was output.
    expect(bus.deliver("t2", new Uint8Array([9]))).toBe(false);
    expect(mine).toEqual([1, 2]);
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

/**
 * Fakes the shell: the six commands and the two events of the contract.
 * **`listen` IS EITHER THERE OR NOT, AND IT IS A PARAMETER**: without the
 * channel the screen must say «non lo so più» instead of «vivo», and unless it
 * can be removed that rule cannot be tested.
 */
function pretendShell(answers: Record<string, unknown>, withEvents = true): FakeShell {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  const handlers = new Map<string, Array<(event: { payload: unknown }) => void>>();
  const calls: Call[] = [];
  const asked: string[] = [];
  const shell: Record<string, unknown> = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        asked.push(command);
        return Promise.resolve(answers[command]);
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
  { id: "t1", workspaceRoot: "/work/sailor", workspaceName: "sailor", alive: true, processId: 4242 },
  { id: "t2", workspaceRoot: "/work/other-repo/packages", workspaceName: "packages", alive: true, processId: 4243 },
];

/**
 * The drawn DOM, measured whole: `:root` carries the roles.
 * **WHOEVER MEASURES MUST BE MEASURED**: the scene declares how many pairs it
 * expects to have found, because a check that looks at nothing passes, and that
 * is exactly how this one would go back to being decoration.
 */
function measure(atLeast: number): string[] {
  const pairs = contrastPairs(document.documentElement, sheet);
  expect(pairs.length).toBeGreaterThanOrEqual(atLeast);
  return belowThreshold(pairs);
}

describe("the terminals screen", () => {
  test("the list comes from `terminal_list`, and every card carries its word", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(shell.asked).toContain("terminal_list");
      expect(screen.getAllByText("vivo").length).toBeGreaterThan(0);
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("A DEAD TERMINAL LOOKS DEAD: `terminal_closed` changes the state shown", async () => {
    // A pane that stays the same when the process inside it has ended. Before
    // the event the card says «vivo»; after the event it says «finito», and the
    // list has not changed yet.
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      expect(screen.queryByText("finito")).toBeNull();

      await act(async () => {
        shell.emit("terminal_closed", { id: "t1", status: "uscita 130" });
      });

      expect(screen.getAllByText("finito").length).toBeGreaterThan(0);
      // The outcome is readable, not just the word: «finito» without knowing how
      // does not say whether the process finished or was killed.
      expect(screen.getByText("uscita 130")).toBeTruthy();
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
      expect(screen.queryByText("vivo")).toBeNull();
      expect(screen.getAllByText("non lo so più").length).toBeGreaterThan(0);
      expect(measure(20)).toEqual([]);
    } finally {
      shell.stop();
    }
  });

  test("OPENING DECLARES THE DIRECTORY: with no workspace nothing opens", async () => {
    // There is no generic terminal you then tell where to go: the directory is
    // part of what the terminal is, and it belongs in the call that opens it.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const button = await screen.findByRole("button", { name: "Apri un terminale" });
      expect((button as HTMLButtonElement).disabled).toBe(true);

      const field = screen.getByPlaceholderText(WORKSPACE_HINT);
      fireEvent.change(field, { target: { value: "/work/sailor" } });
      expect((screen.getByRole("button", { name: "Apri un terminale" }) as HTMLButtonElement).disabled).toBe(false);

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Apri un terminale" }));
      });
      expect(shell.asked).toContain("terminal_open");
    } finally {
      shell.stop();
    }
  });

  test("OPENING CARRIES THE DIRECTORY TO THE COMMAND, not just to the component", async () => {
    // The field is filled in and what reached the bridge is inspected: without
    // this test, `terminal_open` could be called with no `workspaceRoot` and the
    // test above would stay green.
    const shell = pretendShell({ terminal_list: [], terminal_open: TWO[0] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      const field = await screen.findByPlaceholderText(WORKSPACE_HINT);
      fireEvent.change(field, { target: { value: "/work/other-repo" } });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Apri un terminale" }));
      });
      expect(shell.argsOf("terminal_open")).toEqual([
        { workspaceRoot: "/work/other-repo", program: undefined, cols: 80, rows: 24 },
      ]);
    } finally {
      shell.stop();
    }
  });

  test("an empty list is not confused with an engine that does not answer", async () => {
    // «Zero» and «I cannot see» are not the same thing, and they are the two
    // sentences this screen has to be able to choose between.
    const shell = pretendShell({ terminal_list: [] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByText(/Nessun terminale aperto/);
      cleanup();
      render(
        <div className="app">
          <Terminals native={false} />
        </div>,
      );
      expect(screen.getByText(/Non riesco a chiedere/)).toBeTruthy();
    } finally {
      shell.stop();
    }
  });
});

// ── the wiring, not the pieces ───────────────────────────────────────────

/* **THE TWO TESTS THIS SECTION EXISTS FOR.** `keyStroke`, `decodeBytes` and
   `OutputBus` are covered as pure functions, never through the wiring that
   joins them. Here the keys are real events on xterm's textarea, the output is
   the contract's event, and the judge is the bridge call and the emulator. */

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

describe("the wiring between the bridge and the screen", () => {
  test("OUTPUT REACHES THE PANE'S EMULATOR, and a split letter is put back together there", async () => {
    // Guards against bus bytes never reaching the emulator: the screen would
    // stay empty on live output, and none of the pure-function tests would
    // notice.
    const shell = pretendShell({ terminal_list: [TWO[0]] });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });

      await act(async () => {
        // The two events the bridge would send: `è` is 0xC3 0xA8, and a
        // pseudo-terminal may deliver them in two reads.
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xc3])) });
        shell.emit("terminal_output", { id: "t1", bytes: encodeBytes(new Uint8Array([0xa8, 0x21])) });
      });

      await waitFor(() => {
        expect(paneScreenText()).toContain("è!");
      });
    } finally {
      shell.stop();
    }
  });

  test("one terminal's output does not land in another one's pane", async () => {
    const shell = pretendShell({ terminal_list: TWO });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /packages/ });
      await act(async () => {
        shell.emit("terminal_output", { id: "t2", bytes: encodeBytes(keyBytes("nel secondo")) });
      });
      // The first one is the pane on screen, and it must have received nothing.
      await waitFor(() => {
        expect(paneScreenText()).not.toContain("nel secondo");
      });
    } finally {
      shell.stop();
    }
  });

  test("A KEY GOES TO `terminal_press`, ENTER TO `terminal_submit`, AND NEVER BOTH", async () => {
    // Guards against every key being sent **also** to `onSubmit`. This is the
    // fork in the contract, and skipping it made no noise.
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
      await screen.findByRole("button", { name: /sailor/ });
      const keys = keyboardOf();

      // HOW IT IS BORN: A TERMINAL IS BORN A TERMINAL, and the letter is what
      // proves it. An arrow, a Ctrl-C and an Enter would go to `press` while
      // composing too, on an empty line: only the `x` tells the two modes
      // apart, and it is the line that pins the «`raw` by default» decision.
      expect(screen.getByText("i tasti vanno diritti al processo")).toBeTruthy();
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

      // Routing on this line is asked for by whoever wants it.
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "componi una riga da smistare" }));
      });

      // Composing sends nothing to anybody: if even one character left,
      // `terminal_submit` — which writes the line itself — would have it run
      // twice.
      await act(async () => {
        typeLetter(keys, "l");
        typeLetter(keys, "s");
      });
      expect(shell.argsOf("terminal_press").length).toBe(4);
      expect(shell.asked).not.toContain("terminal_submit");

      // Enter: a single call, with the whole line, and not one extra key.
      await act(async () => {
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "ls" }]);
      expect(shell.argsOf("terminal_press").length).toBe(4);

      // And where it ended up can be read on the screen.
      await waitFor(() => {
        expect(screen.getByText(/è andata alla shell/)).toBeTruthy();
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
        text: "trova i residui",
        rule: "marked-request",
      },
    });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });
      const keys = keyboardOf();
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "componi una riga da smistare" }));
      });
      await act(async () => {
        typeLetter(keys, "?");
        tapKey(keys, { key: "Enter", keyCode: 13 });
      });
      expect(shell.argsOf("terminal_submit")).toEqual([{ id: "t1", line: "?" }]);
      // NOTHING REACHED THE SHELL: a rerouted line is not executed.
      expect(shell.asked).not.toContain("terminal_press");
      await waitFor(() => {
        expect(screen.getByText(/marked-request/)).toBeTruthy();
      });
    } finally {
      shell.stop();
    }
  });

  // `terminal_resize` HAS NO TEST, AND HERE IS WHY. The wiring starts from a
  // `ResizeObserver` and from `fit()`, which measure real frames: in jsdom every
  // frame is zero by zero, `fit()` produces no new size and `refit` returns
  // without calling anything. A test here could only assert «zero calls or
  // more», which is true regardless. It is verified by opening the window.

  test("closing a terminal asks the engine, with its id", async () => {
    const shell = pretendShell({ terminal_list: [TWO[0]], terminal_close: null });
    try {
      render(
        <div className="app">
          <Terminals native />
        </div>,
      );
      await screen.findByRole("button", { name: /sailor/ });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Chiudi questo terminale" }));
      });
      expect(shell.argsOf("terminal_close")).toEqual([{ id: "t1" }]);
    } finally {
      shell.stop();
    }
  });
});
