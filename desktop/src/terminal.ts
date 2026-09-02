import type { Step } from "./flow";

// The React half of the terminal contract: the seven commands, the two events,
// and the decisions that can be proved without the bridge.
//
// THE SOURCE IS `docs/2026-09-01-il-contratto-del-terminale.md`, not this
// file. The Rust bridge is written against the same document: if the two
// halves diverge, the document changes, and whoever notices says so instead
// of quietly adapting their own half.
//
// WHY THE DECISIONS LIVE HERE AND NOT IN THE COMPONENTS. Where a key goes,
// how a byte is read, whether a terminal is alive, and how a typed command
// line splits into a program and its arguments are the things this half can
// get wrong on its own. Inside a component they could only be proved by
// drawing the window; here they are pure functions.

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Unlisten = () => void;
type Listen = <T>(event: string, handler: (event: { payload: T }) => void) => Promise<Unlisten>;

interface TauriGlobal {
  core?: { invoke?: Invoke };
  event?: { listen?: Listen };
}

function tauri(): TauriGlobal | null {
  return (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__ ?? null;
}

/**
 * How the shell is called. **A COPY OF `engine.ts`'S, ON PURPOSE**: the
 * contract assigns this work `Terminals.tsx`, `TerminalPane.tsx` and this
 * file, and reaching into `engine.ts` for four lines with no decision in them
 * would touch a file neither half declared. The day `engine.ts` exports its
 * own, this one goes.
 */
function invoker(): Invoke | null {
  return tauri()?.core?.invoke ?? null;
}

/**
 * One row of the list of open terminals.
 *
 * **IT MIRRORS `terminal::Summary`, AND THE TWO HALVES SAY THE SAME THING.**
 * The contract declares camelCase and the crate's type carries it
 * (`#[serde(rename_all = "camelCase")]` in `crates/terminal/src/session.rs`),
 * with its guard and its mutant on the Rust side. No second, lenient shape is
 * read here «to be safe»: a lenient reader would make the halves match while
 * hiding the day they stop matching. If a field arrives under another name,
 * this type must break.
 */
export interface TerminalSummary {
  id: string;
  workspaceRoot: string;
  workspaceName: string;
  alive: boolean;
  processId: number;
  /** The tty of the program inside, short form: the anchor of a tab. */
  device: string;
  /** Bytes moved so far, both ways: the number `sailor terminal list` prints. */
  moved: number;
  /** What those bytes amount to in tokens, by the relay's model: an estimate. */
  estimatedTokens: number;
}

/**
 * The ceiling the relay hands on at, read from the flow that measures: the
 * step whose action measures a terminal declares it in its own `with`. Never a
 * constant here — the budget is a decision written in a flow, and the pane
 * only reports the one it finds. `null` when no loaded flow declares one.
 */
export function declaredCeiling(steps: Step[]): number | null {
  for (const step of steps) {
    if (step.action !== "measure_terminal") continue;
    const ceiling = step.with?.ceiling;
    if (typeof ceiling === "number" && ceiling > 0) return ceiling;
  }
  return null;
}

/**
 * Where a line confirmed with Enter went. The engine decides
 * (`terminal::host::Submitted`), not the window: a second routing rule
 * written here would diverge from the first with nobody noticing. `rule` is
 * the id of the route that recognised the line.
 */
export type Submitted =
  | { kind: "command" }
  | {
      kind: "flow";
      flow: string;
      text: string;
      rule: string;
      /** The run the bridge started with the line as its mandate, if it could. */
      run_id?: string | null;
      /** Why the flow did not start, in the engine's words. */
      refused?: string | null;
    };

/** How a terminal is opened: where, what to start with which arguments, how big. */
export interface Opening {
  /** **The directory is declared at opening, never after.** See `crates/terminal/src/lib.rs`. */
  workspaceRoot: string;
  program?: string;
  args?: string[];
  cols: number;
  rows: number;
}

/** What a terminal printed before this pane looked, and where it ends. */
export interface Backlog {
  at: number;
  bytes: Uint8Array;
  upto: number;
  ended: string | null;
}

// ── the seven commands ───────────────────────────────────────────────────

/**
 * Opens a terminal inside a workspace.
 *
 * **THERE IS NO GENERIC TERMINAL YOU THEN TELL WHERE TO GO.** The directory is
 * part of what the terminal *is*: it is what lets routing know which project
 * is being talked about, and a terminal that discovers its directory after
 * being born belongs, for an instant, to the wrong place — the instant the
 * first line is typed.
 */
export async function openTerminal(opening: Opening): Promise<TerminalSummary> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: no engine to open a terminal");
  return invoke<TerminalSummary>("terminal_open", { ...opening });
}

/**
 * The line confirmed with Enter, looked at **before** it runs: it may go to
 * a flow instead of the shell. On `{ kind: "command" }` the engine has already
 * written it into the pseudo-terminal, newline included, as if typed. On
 * `{ kind: "flow" }` **nothing was written**, and the caller decides.
 */
export async function submitLine(id: string, line: string): Promise<Submitted> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: nobody to hand the line to");
  return invoke<Submitted>("terminal_submit", { id, line });
}

/**
 * Raw bytes on the input, bypassing routing. Bytes and not a string, because
 * the window knows the encoding of what was pressed: turning them into text
 * here and back into bytes there is where an accent gets lost.
 */
export async function pressKeys(id: string, bytes: Uint8Array): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: nobody to send the keys to");
  await invoke<null>("terminal_press", { id, bytes: encodeBytes(bytes) });
}

export async function resizeTerminal(id: string, cols: number, rows: number): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: no terminal to resize");
  await invoke<null>("terminal_resize", { id, cols, rows });
}

export async function closeTerminal(id: string): Promise<void> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: no terminal to close");
  await invoke<null>("terminal_close", { id });
}

/**
 * The open terminals.
 *
 * **IT IS THE ONLY LIST, AND THE WINDOW KEEPS NO COPY.** The terminals are
 * held by a process that outlives the window, and on restart this list is
 * what finds them again. A list kept in the frontend would say «no terminal
 * open» to a machine that has three, with the same face it would wear on a
 * machine that has none.
 */
export async function listTerminals(): Promise<TerminalSummary[]> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: the engine keeps the list of terminals");
  return invoke<TerminalSummary[]>("terminal_list");
}

/**
 * What a terminal printed before this pane looked at it. `upto` is where the
 * live events take over: a pane writes the backlog, then only the events whose
 * `at` is not below it.
 */
export async function terminalBacklog(id: string): Promise<Backlog> {
  const invoke = invoker();
  if (!invoke) throw new Error("outside the desktop shell: no backlog to read");
  const raw = await invoke<{ at: number; bytes: string; upto: number; ended: string | null }>(
    "terminal_backlog",
    { id },
  );
  return { at: raw.at, bytes: decodeBytes(raw.bytes), upto: raw.upto, ended: raw.ended };
}

// ── bytes, which are not text ────────────────────────────────────────────

/**
 * Bytes → base64, **one byte at a time, never through a text string**.
 * `btoa("à")` answers `4A==` — the latin-1 byte 0xE0 — while the same letter in
 * UTF-8 is `0xC3 0xA0`, `w6A=`. A terminal sending the first form writes a
 * different letter into the shell, and only on accented words.
 */
export function encodeBytes(bytes: Uint8Array): string {
  let latin = "";
  for (const byte of bytes) latin += String.fromCharCode(byte);
  return btoa(latin);
}

/**
 * base64 → bytes. **Bytes and not text, which is the whole reason for base64
 * in the contract**: what leaves a pseudo-terminal may be cut in the middle of
 * a multibyte character, and decoding two halves as two strings gives two
 * replacement marks. The emulator puts the letter back together.
 */
export function decodeBytes(base64: string): Uint8Array {
  const latin = atob(base64);
  const bytes = new Uint8Array(latin.length);
  for (let index = 0; index < latin.length; index += 1) bytes[index] = latin.charCodeAt(index);
  return bytes;
}

/** The text the person pressed, in the bytes the shell expects. */
export function keyBytes(data: string): Uint8Array {
  return new TextEncoder().encode(data);
}

// ── what to start ────────────────────────────────────────────────────────

/**
 * A typed command line, split into the program and its arguments.
 *
 * **`claude --resume` STARTS `claude` WITH `--resume`**, not a binary of that
 * name. Quotes group words, as a shell would: `sh -c "echo hi"` is three
 * arguments. Nothing else a shell does — variables, globs, pipes — is done
 * here: the line names a program, it is not run by one.
 */
export function splitCommandLine(text: string): { program: string | undefined; args: string[] } {
  const words: string[] = [];
  let word = "";
  let quote: '"' | "'" | null = null;
  let inWord = false;
  for (const character of text) {
    if (quote !== null) {
      if (character === quote) quote = null;
      else word += character;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      inWord = true;
      continue;
    }
    if (character === " " || character === "\t") {
      if (inWord) words.push(word);
      word = "";
      inWord = false;
      continue;
    }
    word += character;
    inWord = true;
  }
  if (inWord) words.push(word);
  const [program, ...args] = words;
  return { program, args };
}

// ── where a key goes ─────────────────────────────────────────────────────

/**
 * What the keyboard is doing right now. `compose` is the terminal at a
 * prompt: the window holds the line, draws it, and hands it to routing on
 * Enter. `raw` is the terminal inside a program: every key goes through as it
 * is, Enter included.
 */
export type KeyMode = "compose" | "raw";

/** What to do with a key. A decision, not an effect: the effects are the pane's. */
export type KeyAction =
  /** To the shell, raw bytes. */
  | { kind: "press"; bytes: Uint8Array }
  /** To routing, as a whole line. */
  | { kind: "submit"; line: string }
  /** On screen, drawn by the window: the echo of the line being composed. */
  | { kind: "echo"; text: string }
  /** Nowhere, and it says why. */
  | { kind: "ignored"; why: string };

export interface Stroke {
  /** The line being composed after this key. */
  draft: string;
  actions: KeyAction[];
}

/** Enter, in both forms an emulator delivers it. */
function isEnter(data: string): boolean {
  return data === "\r" || data === "\n" || data === "\r\n";
}

/** Erases the last character: back, space, back. */
function eraseCells(count: number): string {
  return "\b \b".repeat(count);
}

/**
 * Everything printable, a paste of several characters included. A control
 * character in the middle excludes it: a line is being composed, not a
 * program driven.
 */
function isPrintable(data: string): boolean {
  if (data.length === 0) return false;
  for (const character of data) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return false;
  }
  return true;
}

/**
 * **ENTER AND KEYS ARE TWO DIFFERENT ROADS, AND THIS FUNCTION IS THE FORK.**
 *
 * Only the line confirmed with Enter leaves as `submit`; everything else
 * leaves as `press`. If every key went through routing an editor inside the
 * terminal would be unusable.
 *
 * **WHY COMPOSE THE LINE INSTEAD OF MIRRORING IT.** `terminal_submit` writes
 * the line into the pseudo-terminal itself when it is a command. Had the
 * window already sent those characters, the line would run twice — `lsls`. So
 * while composing **nothing leaves**: the window draws the echo and erases it
 * on Enter, and the shell writes the line once.
 *
 * **THE PRICE, DECLARED — AND WHY `raw` IS THE DEFAULT.** While the window
 * holds the line the shell's `readline` does not: history, arrows and above
 * all Tab are lost, and nothing tells the window a full-screen program just
 * started in there. A terminal is therefore **born a terminal**: `compose` is
 * the explicit choice of whoever wants routing on that line.
 *
 * Inside `compose` a key that is not a character, a backspace or Enter **does
 * not leave** and says why. On an empty line the terminal is a passthrough.
 * Ctrl-C is the exception to everything and always goes through: a way to
 * stop what runs is taken from nobody.
 */
export function keyStroke(mode: KeyMode, draft: string, data: string): Stroke {
  if (mode === "raw") {
    return { draft: "", actions: [{ kind: "press", bytes: keyBytes(data) }] };
  }

  // Ctrl-C before anything: it cancels the line being composed and still
  // reaches whatever is running.
  if (data === "\x03") {
    return {
      draft: "",
      actions: [
        { kind: "echo", text: eraseCells(draft.length) },
        { kind: "press", bytes: keyBytes(data) },
      ],
    };
  }

  if (isEnter(data)) {
    // An empty Enter is a newline, not a line to route: sending it to routing
    // would ask a set of rules what to do with nothing.
    if (draft === "") return { draft: "", actions: [{ kind: "press", bytes: keyBytes("\r") }] };
    return {
      draft: "",
      actions: [
        { kind: "echo", text: eraseCells(draft.length) },
        { kind: "submit", line: draft },
      ],
    };
  }

  if (data === "\x7f" || data === "\b") {
    if (draft === "") return { draft, actions: [{ kind: "ignored", why: "there is nothing to erase" }] };
    return { draft: draft.slice(0, -1), actions: [{ kind: "echo", text: eraseCells(1) }] };
  }

  // Ctrl-U: the line is thrown away, and nobody runs it.
  if (data === "\x15") {
    return { draft: "", actions: [{ kind: "echo", text: eraseCells(draft.length) }] };
  }

  if (isPrintable(data)) {
    return { draft: draft + data, actions: [{ kind: "echo", text: data }] };
  }

  if (draft === "") {
    return { draft, actions: [{ kind: "press", bytes: keyBytes(data) }] };
  }

  return {
    draft,
    actions: [
      {
        kind: "ignored",
        why: "while the window holds the line only characters, backspace and Enter go through: empty it to use the shell's keys",
      },
    ],
  };
}

// ── alive, ended, or no longer known ─────────────────────────────────────

/**
 * How a terminal is doing, for whoever looks.
 *
 * **THREE STATES, NOT TWO.** «Alive» and «no longer known» are not the same
 * thing: if the event channel did not attach, a death would never arrive, and
 * a pane that keeps saying «alive» is asserting something it cannot know.
 */
export type Liveness =
  | { state: "alive" }
  | { state: "closed"; status: string | null }
  | { state: "unknown"; why: string };

/**
 * The state to show, from the only two facts the window has. The order
 * matters: **an event wins over the list**, because `terminal_closed` arrived
 * when the process ended while the list is as old as the last poll.
 */
export function livenessOf(
  summary: TerminalSummary,
  closed: ReadonlyMap<string, string>,
  watching: boolean,
): Liveness {
  const status = closed.get(summary.id);
  if (status !== undefined) return { state: "closed", status };
  // The list calls it closed without saying how: a poorer fact than the
  // event, written for what it is instead of inventing a state.
  if (!summary.alive) return { state: "closed", status: null };
  if (!watching) {
    return {
      state: "unknown",
      why: "the event channel is not there: if the process ended, this pane would not know",
    };
  }
  return { state: "alive" };
}

/** The word beside the colour: colour alone does not carry state. */
export function livenessWord(liveness: Liveness): string {
  switch (liveness.state) {
    case "alive":
      return "alive";
    case "closed":
      return "ended";
    case "unknown":
      return "no longer known";
  }
}

// ── the output, as it comes ──────────────────────────────────────────────

/** A piece of output, with the offset of its first byte. */
export type OutputReader = (bytes: Uint8Array, at: number) => void;

/**
 * Whoever draws a terminal subscribes here; whoever listens to the event
 * pours in. **Not React state**: what leaves a pseudo-terminal arrives in
 * pieces and continuously, and a `setState` per piece would redraw the window
 * on every line of a build. The bytes go to the emulator, which is the only
 * one that knows what to do with them.
 */
export class OutputBus {
  private readonly readers = new Map<string, OutputReader>();

  /** Returns how to unsubscribe. */
  subscribe(id: string, reader: OutputReader): () => void {
    this.readers.set(id, reader);
    return () => {
      if (this.readers.get(id) === reader) this.readers.delete(id);
    };
  }

  /**
   * Delivers. **Returns false if nobody was watching**, and it is not a
   * detail: bytes for a terminal with no pane are lost bytes, and whoever
   * loses them in silence shows an empty screen where there was output.
   */
  deliver(id: string, bytes: Uint8Array, at: number): boolean {
    const reader = this.readers.get(id);
    if (!reader) return false;
    reader(bytes, at);
    return true;
  }
}

interface Watchers {
  onOutput: (id: string, bytes: Uint8Array, at: number) => void;
  onClosed: (id: string, status: string) => void;
}

/**
 * Listens to the two events of the contract.
 *
 * **A LISTENER THAT DOES NOT ATTACH MUST SAY SO**, like `listenToRuns`:
 * returning `null` in silence once cost a view showing «running» on a run
 * long finished. Here it would cost more — a dead terminal drawn alive — so
 * the reason goes back to the caller, who feeds it into `livenessOf`.
 *
 * **BOTH OR NEITHER.** If the second listener does not attach, the first is
 * undone: half a channel is output flowing onto a pane that will never know
 * it died, which is worse than no channel because it looks like it works.
 */
export async function watchTerminals(
  watchers: Watchers,
): Promise<{ stop: () => void } | { why: string }> {
  const shell = tauri();
  if (!shell) return { why: "outside the desktop shell: no event channel" };
  const listen = shell.event?.listen;
  if (!listen) {
    return { why: "the shell exposes no «event.listen»: a dead terminal would stay drawn alive" };
  }
  try {
    const stopOutput = await listen<{ id: string; bytes: string; at: number }>("terminal_output", (event) => {
      watchers.onOutput(event.payload.id, decodeBytes(event.payload.bytes), event.payload.at);
    });
    try {
      const stopClosed = await listen<{ id: string; status: string }>("terminal_closed", (event) => {
        watchers.onClosed(event.payload.id, event.payload.status);
      });
      return {
        stop: () => {
          stopOutput();
          stopClosed();
        },
      };
    } catch (error) {
      stopOutput();
      return { why: String(error) };
    }
  } catch (error) {
    return { why: String(error) };
  }
}
