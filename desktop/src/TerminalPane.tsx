// A terminal, drawn.
//
// **THE EMULATOR IS NOT OURS, AND THAT IS A WRITTEN CHOICE.** Interpreting ANSI
// sequences by hand is a project of its own: cursor positioning, the alternate
// screen, 256 colours, double-width characters, sequences cut between two
// reads. `@xterm/xterm` does that piece, has for ten years, and is the emulator
// inside VS Code. Product direction 3 of this repository says exactly this: if
// a live project does that piece, connect it.
//
// **THIS FILE DECIDES NOTHING.** Where a key goes, how a byte is read and
// whether a terminal is alive live in `terminal.ts`, as pure functions. Here is
// only the wiring between those decisions and what is seen — deliberately: a
// component that mounts an emulator can only be proved by drawing it, and what
// can be proved without drawing must not live inside it.

import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as Emulator } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  keyStroke,
  livenessWord,
  terminalBacklog,
  type KeyMode,
  type Liveness,
  type OutputBus,
  type Submitted,
  type TerminalSummary,
} from "./terminal";

/**
 * How big a terminal is born before anybody measures it. Not a preference:
 * `terminal_open` wants `cols` and `rows`, and the real size arrives after the
 * first paint. Eighty by twenty-four is what every full-screen program handles.
 */
export const BORN_COLS = 80;
export const BORN_ROWS = 24;

interface PaneProps {
  summary: TerminalSummary;
  /** The ceiling the relay hands on at, or `null` when no loaded flow declares one. */
  ceiling: number | null;
  liveness: Liveness;
  /** Where the process's bytes arrive: the pane subscribes for its own `id`. */
  bus: OutputBus;
  /** Hidden when the screen is away: the emulator stays alive and keeps receiving. */
  visible: boolean;
  /** Drawn large and first: the one the person is looking at. */
  focused: boolean;
  onFocus: () => void;
  /** The line confirmed with Enter. Returns where it went, and the pane writes it. */
  onSubmit: (line: string) => Promise<Submitted>;
  onPress: (bytes: Uint8Array) => void;
  onResize: (cols: number, rows: number) => void;
}

export function TerminalPane({
  summary,
  ceiling,
  liveness,
  bus,
  visible,
  focused,
  onFocus,
  onSubmit,
  onPress,
  onResize,
}: PaneProps) {
  /** The line under the pane, held by the window until Enter. */
  const [asked, setAsked] = useState("");
  const host = useRef<HTMLDivElement | null>(null);
  const emulator = useRef<Emulator | null>(null);
  const fitter = useRef<FitAddon | null>(null);
  /** The line being composed lives in a `ref`: the keyboard arrives outside React. */
  const draft = useRef("");
  const [shown, setShown] = useState("");
  // A TERMINAL IS BORN A TERMINAL. The default mode is `raw` because composing
  // the line costs Tab and the full-screen programs: see the price written in
  // full above `keyStroke`.
  const [mode, setMode] = useState<KeyMode>("raw");
  const [routed, setRouted] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  /** Why the backlog could not be shown, when it could not. */
  const [missing, setMissing] = useState<string | null>(null);

  // The handlers change on every render, the emulator mounts once: in the
  // mount's dependencies every render would throw the terminal away, and with
  // it everything that came out inside.
  const latest = useRef({ onSubmit, onPress, onResize, mode });
  latest.current = { onSubmit, onPress, onResize, mode };

  useEffect(() => {
    const where = host.current;
    if (!where) return;
    const term = new Emulator({
      cols: BORN_COLS,
      rows: BORN_ROWS,
      convertEol: false,
      fontFamily: readToken("--font-data") || "monospace",
      fontSize: 12,
      theme: themeFromTokens(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(where);
    emulator.current = term;
    fitter.current = fit;

    const typed = term.onData((data) => {
      const stroke = keyStroke(latest.current.mode, draft.current, data);
      draft.current = stroke.draft;
      setShown(stroke.draft);
      for (const action of stroke.actions) {
        switch (action.kind) {
          case "echo":
            if (action.text !== "") term.write(action.text);
            break;
          case "press":
            setRefused(null);
            latest.current.onPress(action.bytes);
            break;
          case "submit":
            setRefused(null);
            void latest.current
              .onSubmit(action.line)
              .then((answer) => setRouted(routingNote(action.line, answer)))
              .catch((error: unknown) => setRefused(String(error)));
            break;
          case "ignored":
            // A KEY THAT DOES NOT LEAVE SAYS SO. Silence and «it did not work»
            // look too much alike, and on the second one presses again.
            setRefused(action.why);
            break;
        }
      }
    });

    // The frame changes when the window does, not when React decides.
    let watcher: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      watcher = new ResizeObserver(() => refit(fit, term, latest.current.onResize));
      watcher.observe(where);
    }

    return () => {
      typed.dispose();
      watcher?.disconnect();
      term.dispose();
      emulator.current = null;
      fitter.current = null;
    };
  }, []);

  // ATTACHING LATE: the backlog first, then only the live pieces that follow
  // it. The bytes go to the emulator and not to React state — a `setState`
  // per piece would redraw the window on every line of a build — and they stay
  // `Uint8Array` to the end, so an accent split across two events is put back
  // together in there. Until the backlog has arrived the live pieces are held,
  // and the offset each carries says whether the backlog already had it.
  useEffect(() => {
    const held: Array<{ bytes: Uint8Array; at: number }> = [];
    let upto: number | null = null;
    const unsubscribe = bus.subscribe(summary.id, (bytes, at) => {
      if (upto === null) {
        held.push({ bytes, at });
        return;
      }
      if (at >= upto) emulator.current?.write(bytes);
    });
    let gone = false;
    terminalBacklog(summary.id)
      .then((backlog) => {
        if (gone) return;
        emulator.current?.write(backlog.bytes);
        upto = backlog.upto;
        for (const piece of held) if (piece.at >= upto) emulator.current?.write(piece.bytes);
        held.length = 0;
        setMissing(null);
      })
      .catch((error: unknown) => {
        if (gone) return;
        // Without the backlog the live output still flows, and the pane says
        // what it could not show instead of passing an empty screen for one
        // where nothing happened.
        upto = 0;
        for (const piece of held) emulator.current?.write(piece.bytes);
        held.length = 0;
        setMissing(String(error));
      });
    return () => {
      gone = true;
      unsubscribe();
    };
  }, [bus, summary.id]);

  // A hidden pane has no width: when it comes back it is measured again, or
  // it keeps the size it had when it was born.
  useEffect(() => {
    if (!visible) return;
    const term = emulator.current;
    const fit = fitter.current;
    if (term && fit) refit(fit, term, latest.current.onResize);
  }, [visible]);

  const dead = liveness.state === "closed";

  return (
    <section className="pane" hidden={!visible} data-focus={focused || undefined}>
      <header className="pane__head" onClick={onFocus}>
        {/* THE TTY FIRST: it is what this session *is* to everything else on
            the machine — the letterbox, the count, the tracking store. */}
        <span className="pane__device">{summary.device}</span>
        <span className="label">{summary.workspaceName}</span>
        <span className="pane__root">{summary.workspaceRoot}</span>
        {/* The word sits beside the colour: prohibition 5. */}
        <span className="pane__state" data-state={liveness.state}>
          {livenessWord(liveness)}
        </span>
        {liveness.state === "unknown" && <span className="pane__why">{liveness.why}</span>}
        {liveness.state === "closed" && liveness.status !== null && (
          <span className="pane__why">{liveness.status}</span>
        )}
        <span className="pane__moved">{movedLabel(summary.moved)}</span>
        {/* The number the relay compares to its ceiling, next to the bytes it
            is made from: an estimate, and marked as one. */}
        <span
          className="pane__tokens"
          data-past={ceiling !== null && summary.estimatedTokens >= ceiling ? "true" : undefined}
        >
          {tokensLabel(summary.estimatedTokens, ceiling)}
        </span>
        <span className="pane__id">{summary.id}</span>
        {/* THE STATE AND THE GESTURE ARE TWO THINGS. One label on a button does
            not say whether it names how things are now or what happens on
            pressing it, and the keyboard mode is the costliest thing to get
            wrong here. */}
        <span className="pane__keys">
          {mode === "compose" ? "the window holds the line" : "keys go straight to the process"}
        </span>
        <button
          type="button"
          className="pane__mode"
          data-mode={mode}
          onClick={() => setMode(mode === "compose" ? "raw" : "compose")}
          disabled={dead}
        >
          {mode === "compose" ? "back to direct keys" : "compose a line to route"}
        </button>
      </header>

      {/* THE EMULATOR LIVES IN HERE, AND STAYS MOUNTED EVEN DEAD: what the
          process wrote before ending is the part one goes back to read, and
          unmounting it would erase it. */}
      <div className="pane__screen" ref={host} />

      {/* THE LINE UNDER THE PANE: a command for the shell, or a question for
          a flow; the router decides which. The keys inside the emulator stay
          the program's, so an editor or an agent never sees this line. */}
      <form
        className="pane__ask"
        onSubmit={(event) => {
          event.preventDefault();
          const line = asked.trim();
          if (line === "" || dead) return;
          setAsked("");
          setRefused(null);
          void onSubmit(line)
            .then((answer) => setRouted(routingNote(line, answer)))
            .catch((error: unknown) => setRefused(String(error)));
        }}
      >
        <span className="pane__prompt" aria-hidden="true">
          ›
        </span>
        <input
          className="pane__ask-line"
          aria-label={`a line for ${summary.device}`}
          placeholder="type a command, or ask about a flow"
          value={asked}
          disabled={dead}
          onFocus={onFocus}
          onChange={(event) => setAsked(event.target.value)}
        />
      </form>

      <footer className="pane__foot">
        {mode === "compose" ? (
          <span className="pane__draft" data-empty={shown === "" || undefined}>
            {shown === "" ? "type a line; Enter sends it to routing" : shown}
          </span>
        ) : (
          <span className="pane__draft" data-empty>
            every key goes to the process as it is, Enter included
          </span>
        )}
        {missing !== null && (
          <span className="pane__refused" data-gravity="warn">
            what this terminal printed earlier could not be shown: {missing}
          </span>
        )}
        {routed !== null && <span className="pane__routed">{routed}</span>}
        {refused !== null && (
          <span className="pane__refused" data-gravity="warn">
            {refused}
          </span>
        )}
      </footer>
    </section>
  );
}

/**
 * Where the line ended up, told to whoever is watching.
 *
 * **A REROUTED LINE IS SEEN, AND SO IS THE RULE THAT REROUTED IT.** A terminal
 * that now and then does not run what you type is worse than one that does
 * not route at all: it becomes unpredictable, and unpredictability is paid on
 * every line typed after. The rule's name leads back to the line of JSON that
 * decided, not only to the flow.
 */
export function routingNote(line: string, answer: Submitted): string {
  if (answer.kind === "command") return `«${line}» went to the shell`;
  const sent = `rule «${answer.rule}» sent «${line}» to flow «${answer.flow}» as «${answer.text}»`;
  if (answer.run_id) return `${sent}: run ${answer.run_id} started`;
  if (answer.refused) return `${sent}, and the flow did not start: ${answer.refused}`;
  return `${sent}; nothing ran it`;
}

/**
 * The bytes moved so far, in the unit a person reads. The same number
 * `sailor terminal list` prints, before it turns it into tokens: what the
 * relay measures to say when this session is about to need the baton.
 */
export function movedLabel(moved: number): string {
  if (moved < 1024) return `${String(moved)} bytes moved`;
  if (moved < 1024 * 1024) return `${String(Math.round(moved / 1024))} KB moved`;
  return `${(moved / (1024 * 1024)).toFixed(1)} MB moved`;
}

/** A token count in the short form a header has room for: `840`, `62k`, `1.2M`. */
function shortCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${String(Math.round(n / 1000))}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/**
 * The estimate against the ceiling, when a flow declares one. The `≈` is not
 * decoration: the count is fitted from bytes, and a header that wrote it as a
 * measurement would be claiming what nobody measured.
 */
export function tokensLabel(estimated: number, ceiling: number | null): string {
  if (ceiling === null) return `≈ ${shortCount(estimated)} tokens`;
  return `≈ ${shortCount(estimated)} of ${shortCount(ceiling)} tokens`;
}

/** Measures again, and tells the engine the new size only if it really changed. */
function refit(fit: FitAddon, term: Emulator, tell: (cols: number, rows: number) => void): void {
  const before = { cols: term.cols, rows: term.rows };
  try {
    fit.fit();
  } catch {
    // A zero-wide frame — a hidden pane, the window shrunk to nothing — is not
    // a failure: it is a measure that cannot be taken right now.
    return;
  }
  if (term.cols !== before.cols || term.rows !== before.rows) tell(term.cols, term.rows);
}

/** The value of a stylesheet role, read from the document. */
function readToken(name: string): string {
  if (typeof getComputedStyle !== "function") return "";
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/**
 * The emulator's colours come from the roles, not from a palette of its own:
 * the plate is ink with paper letters, as code is drawn everywhere else, and
 * prohibition 4 reserves colour for the machine's state. If a role cannot be
 * read — outside a real browser — nothing is invented.
 */
function themeFromTokens(): { background?: string; foreground?: string; cursor?: string } {
  const background = readToken("--ink-surface");
  const foreground = readToken("--on-ink");
  if (background === "" || foreground === "") return {};
  return { background, foreground, cursor: foreground };
}
