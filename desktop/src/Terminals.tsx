// The terminals: the tabs, the panes, and who opens the next one.

// **THE LIST BELONGS TO THE ENGINE, NOT TO THIS SCREEN.** `terminal_list` is
// the only answer to "which terminals exist", and this component keeps no copy.
// The terminals are held by `sailor terminal host`, a process that outlives
// this window: a list kept here would say "none" on the first paint of a
// window whose machine has three, wearing the same face it would wear on a
// machine that has none.

// **WHAT THIS SCREEN REALLY OWNS** are three things the engine cannot know:
// which tab you are watching, which terminals have sent `terminal_closed` since
// you arrived, and whether the event channel attached. The third is why a
// "no longer known" state exists at all.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAsk } from "./ask";
import { ChangesScreen } from "./ChangesScreen";
import { BORN_COLS, BORN_ROWS, TerminalPane } from "./TerminalPane";
import {
  closeTerminal,
  listTerminals,
  livenessOf,
  livenessWord,
  openTerminal,
  OutputBus,
  pressKeys,
  resizeTerminal,
  splitCommandLine,
  submitLine,
  watchTerminals,
  type TerminalSummary,
} from "./terminal";
import { commandLines, type CommandLine } from "./profiles";
import { projects, type Project } from "./workspaces";
import { listTrees, type Tree } from "./worktree";

/** How often the list is re-asked. A terminal is born and dies by hand. */
const REFRESH_MS = 4000;

interface TerminalsProps {
  /** True inside the native shell: outside there is no pty to open. */
  native: boolean;
  /**
   * False while another place is on screen. The screen stays mounted, hidden:
   * unmounting it would destroy every emulator, and a session would come back
   * blank while the process inside is alive and talking.
   */
  shown?: boolean;
  /** The ceiling the relay hands on at, from the flow that declares it; `null` when none does. */
  ceiling?: number | null;
  /** How many terminals are open, told whenever it changes. */
  onCount?: (count: number) => void;
  /** The list itself, for the column that nests each terminal under its tree. */
  onList?: (all: TerminalSummary[]) => void;
}

/**
 * The placeholder of the path field. **IT NAMES NO REAL MACHINE**: a
 * developer's absolute path in a product field breaks the charter, and a gate
 * finds one. Exported so the test reads it here, not a copy.
 */
export const WORKSPACE_HINT = "/path/to/your/project";

/** The choice that means «a path typed by hand» instead of a known place. */
export const ANOTHER_PATH = "…another path";

/** A place a terminal can be opened in, as the engine names it. */
export interface Place {
  root: string;
  label: string;
  /** Whether this is the one being worked in right now. */
  current?: boolean;
}

/**
 * The known workspaces and worktrees, as the engine answers them. **NOT A
 * LIST KEPT HERE**: `workspaces` and `worktree_list` already answer the
 * question, and a screen that remembered places would offer a project that
 * moved yesterday. A root that is both stays one entry.
 */
export function placesOf(known: Project[], trees: Tree[]): Place[] {
  const places: Place[] = [];
  const seen = new Set<string>();
  for (const project of known) {
    if (seen.has(project.root)) continue;
    seen.add(project.root);
    places.push({ root: project.root, label: `${project.name} · project`, current: project.current });
  }
  for (const tree of trees) {
    if (seen.has(tree.path)) continue;
    seen.add(tree.path);
    places.push({
      root: tree.path,
      label: `${tree.name} · worktree${tree.branch ? ` on ${tree.branch}` : ""}`,
      current: tree.current,
    });
  }
  return places;
}

/** One array, so a render with nothing open does not tell the column anew. */
const EMPTY: TerminalSummary[] = [];

export function Terminals({ native, shown = true, ceiling = null, onCount, onList }: TerminalsProps) {
  const outside = "outside the desktop shell: pseudo-terminals are the engine's to open";
  const { asked, again } = useAsk<TerminalSummary[]>(native, listTerminals, REFRESH_MS, outside);
  const openedCount = asked.state === "answered" ? asked.value.length : 0;
  const known = asked.state === "answered" ? asked.value : EMPTY;
  useEffect(() => {
    onCount?.(openedCount);
  }, [openedCount, onCount]);
  useEffect(() => {
    onList?.(known);
  }, [known, onList]);

  const [here, setHere] = useState<string | null>(null);
  /** Who sent `terminal_closed`, and with what outcome. */
  const [closed, setClosed] = useState<Map<string, string>>(() => new Map());
  /** The event channel: attached, or the reason it is not. */
  const [channel, setChannel] = useState<{ on: boolean; why: string | null }>({ on: false, why: null });
  const [opening, setOpening] = useState(false);
  /** Whether the opening form is asking, rather than using what it knows. */
  const [detailed, setDetailed] = useState(false);
  /** The chosen place's root, `ANOTHER_PATH`, or "" before anything was chosen. */
  const [chosen, setChosen] = useState("");
  const [typed, setTyped] = useState("");
  const [program, setProgram] = useState("");
  const [trouble, setTrouble] = useState<string | null>(null);
  /** Bytes arrived for a terminal with no pane: they would be lost output. */
  const [orphans, setOrphans] = useState(0);
  /** Whether what changed in the visible terminal's workspace is on screen. */
  const [reading, setReading] = useState(false);
  const [places, setPlaces] = useState<Place[]>([]);
  /** The command lines this machine knows, as what a terminal can be born on. */
  const [lines, setLines] = useState<CommandLine[]>([]);
  /** Why the known places could not all be read, when they could not. */
  const [placesWhy, setPlacesWhy] = useState<string | null>(null);

  const bus = useMemo(() => new OutputBus(), []);

  /* WHICH ONES ARE TALKING RIGHT NOW. The bytes stay out of React — a
     `setState` per piece would redraw the window on every line of a build —
     so what crosses over is only this set, a few times a second at most. */
  const [speaking, setSpeaking] = useState<ReadonlySet<string>>(new Set());
  useEffect(() => bus.watchSpeaking((now) => setSpeaking(now)), [bus]);
  // `again` changes identity on every render; the listener attaches once.
  const refresh = useRef(again);
  refresh.current = again;

  useEffect(() => {
    if (!native) return;
    let dropped = false;
    void Promise.allSettled([projects(), listTrees()]).then(([known, trees]) => {
      if (dropped) return;
      setPlaces(
        placesOf(
          known.status === "fulfilled" ? known.value : [],
          trees.status === "fulfilled" ? trees.value : [],
        ),
      );
      const refused = [known, trees]
        .filter((outcome) => outcome.status === "rejected")
        .map((outcome) => String((outcome as PromiseRejectedResult).reason));
      setPlacesWhy(refused.length > 0 ? refused.join("; ") : null);
    });
    return () => {
      dropped = true;
    };
  }, [native]);

  // Read once: the table of command lines is what the product knows, not what
  // this machine has, so it does not go stale while a terminal is open.
  useEffect(() => {
    if (!native) return;
    let dropped = false;
    void commandLines().then(
      (known) => {
        if (!dropped) setLines(known);
      },
      // Not being able to list them costs the buttons, and nothing else: the
      // field below still takes any command line.
      () => {},
    );
    return () => {
      dropped = true;
    };
  }, [native]);

  useEffect(() => {
    if (!native) {
      setChannel({ on: false, why: outside });
      return;
    }
    let stop: (() => void) | null = null;
    let dropped = false;
    void watchTerminals({
      onOutput: (id, bytes, at) => {
        if (!bus.deliver(id, bytes, at)) setOrphans((seen) => seen + 1);
      },
      onClosed: (id, status) => {
        setClosed((before) => new Map(before).set(id, status));
        // The list also carries `alive`: it is re-asked at once, so the two
        // sources agree again instead of diverging until the next poll.
        refresh.current();
      },
    }).then((outcome) => {
      if ("why" in outcome) {
        setChannel({ on: false, why: outcome.why });
        return;
      }
      if (dropped) {
        outcome.stop();
        return;
      }
      stop = outcome.stop;
      setChannel({ on: true, why: null });
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [native, bus, outside]);

  // Nothing chosen yet means the first known place, or a typed path when
  // there is none: the form is never blank on a machine with projects.
  // WHERE YOU ARE STANDING, not the head of a list. A terminal opened in the
  // wrong tree is discovered at the first command that reads a file.
  const here_ = places.find((place) => place.current) ?? places[0];
  const choice = chosen === "" ? (here_?.root ?? ANOTHER_PATH) : chosen;
  const root = choice === ANOTHER_PATH ? typed.trim() : choice;

  const open = useCallback(async () => {
    setTrouble(null);
    setOpening(true);
    try {
      const line = splitCommandLine(program);
      const born = await openTerminal({
        workspaceRoot: root,
        program: line.program,
        args: line.args.length > 0 ? line.args : undefined,
        cols: BORN_COLS,
        rows: BORN_ROWS,
      });
      setHere(born.id);
      again();
    } catch (error) {
      // The engine's error has to be read: a missing folder and a shell that
      // will not start are fixed in two different ways.
      setTrouble(String(error));
    } finally {
      setOpening(false);
    }
  }, [root, program, again]);

  if (asked.state === "mute") {
    return (
      <div className="terminals" hidden={!shown}>
        <p className="terminals__mute">I cannot ask which terminals are open: {asked.why}</p>
      </div>
    );
  }

  if (asked.state === "asking") {
    return (
      <div className="terminals" hidden={!shown}>
        <p className="terminals__mute">Asking the engine which terminals are open…</p>
      </div>
    );
  }

  const opened = asked.value;
  const visible = opened.some((entry) => entry.id === here) ? here : (opened[0]?.id ?? null);
  const watched = opened.find((entry) => entry.id === visible) ?? null;

  return (
    <div className="terminals" hidden={!shown}>
      {/* THE DIRECTORY IS DECLARED AT OPENING. There is no generic terminal you
          then tell where to go: the workspace is part of what the terminal is,
          and what lets routing know which project is being talked about. */}
      {/* ONE GESTURE WHEN THERE IS NOTHING TO CHOOSE. With a terminal already
          open the answers are known — the tree you are standing in, the shell
          you used — and a form asking them again on every new terminal is a
          form asking you to confirm what it already knows. */}
      <form
        className="terminals__open"
        data-asking={detailed || opened.length === 0 || undefined}
        onSubmit={(event) => {
          event.preventDefault();
          void open();
        }}
      >
        {(detailed || opened.length === 0) && (
        <>
        <label className="terminals__field">
          <span className="label">Workspace</span>
          <select className="terminals__select" value={choice} onChange={(event) => setChosen(event.target.value)}>
            {places.map((place) => (
              <option key={place.root} value={place.root}>
                {place.label}
              </option>
            ))}
            <option value={ANOTHER_PATH}>{ANOTHER_PATH}</option>
          </select>
        </label>
        {choice === ANOTHER_PATH && (
          <label className="terminals__field">
            <span className="label">Path</span>
            <input
              className="terminals__input"
              value={typed}
              placeholder={WORKSPACE_HINT}
              onChange={(event) => setTyped(event.target.value)}
            />
          </label>
        )}
        <label className="terminals__field">
          <span className="label">What to start</span>
          {/* THE ENGINES ARE OFFERED, NOT TYPED FROM MEMORY. Their names come
              from the table of command lines, so a machine with other engines
              on it offers those, and this screen names none of them. */}
          {lines.length > 0 && (
            <span className="terminals__engines">
              <button
                type="button"
                className="terminals__engine"
                data-chosen={program.trim() === "" || undefined}
                onClick={() => setProgram("")}
              >
                your shell
              </button>
              {lines.map((line) => (
                <button
                  key={line.id}
                  type="button"
                  className="terminals__engine"
                  data-chosen={program.trim() === line.executable || undefined}
                  onClick={() => setProgram(line.executable)}
                >
                  {line.display_name}
                </button>
              ))}
            </span>
          )}
          <input
            className="terminals__input"
            value={program}
            placeholder="your shell, or a command line with its options"
            onChange={(event) => setProgram(event.target.value)}
          />
        </label>
        </>
        )}
        <button type="submit" className="is-primary" disabled={opening || root === ""}>
          {opening ? "opening…" : opened.length === 0 ? "Open a terminal" : "New terminal"}
        </button>
        {opened.length > 0 && (
          <button
            type="button"
            className="terminals__elsewhere"
            onClick={() => setDetailed((was) => !was)}
          >
            {detailed ? "never mind" : "somewhere else…"}
          </button>
        )}
      </form>

      {placesWhy !== null && (
        <p className="terminals__trouble" data-gravity="warn">
          The known workspaces could not all be read: {placesWhy}. A path can still be typed.
        </p>
      )}

      {trouble !== null && (
        <p className="terminals__trouble" data-gravity="danger">
          {trouble}
        </p>
      )}

      {channel.why !== null && (
        <p className="terminals__trouble" data-gravity="warn">
          {channel.why} — until then these panes receive neither the output nor the end of a process.
        </p>
      )}

      {orphans > 0 && (
        <p className="terminals__trouble" data-gravity="warn">
          {orphans} pieces of output arrived for a terminal with no pane, and were lost.
        </p>
      )}

      {opened.length === 0 ? (
        <p className="terminals__empty">No terminal is open. That is not the same as being unable to ask.</p>
      ) : (
        <>
          <nav className="terminals__tabs">
            {opened.map((entry) => {
              const liveness = livenessOf(entry, closed, channel.on);
              return (
                <button
                  key={entry.id}
                  type="button"
                  className="terminals__tab"
                  data-here={entry.id === visible || undefined}
                  data-state={liveness.state}
                  onClick={() => setHere(entry.id)}
                >
                  {/* THE TTY IS THE ANCHOR: not a product name, not a title
                      read out of the output. It is what the letterbox, the
                      count and the tracking store all key on. */}
                  <span className="terminals__device">{entry.device}</span>
                  <span className="terminals__where">{entry.workspaceName}</span>
                  {/* The word carries the state as much as the colour: prohibition 5,
                      and it is what is left when motion is refused. */}
                  {liveness.state === "alive" && speaking.has(entry.id) && (
                    <span className="speaks" aria-hidden="true" />
                  )}
                  <span className="terminals__word">
                    {livenessWord(liveness, speaking.has(entry.id))}
                  </span>
                </button>
              );
            })}
          </nav>

          {/* EVERY TERMINAL IS ON SCREEN AT ONCE. A day is spent watching two
              agents, and one pane behind tabs meant flipping between them; the
              one in focus is drawn large and first, the others beside it. */}
          <div className="terminals__panes" data-count={opened.length}>
            {[...opened]
              .sort((a, b) => Number(b.id === visible) - Number(a.id === visible))
              .map((entry) => {
              const liveness = livenessOf(entry, closed, channel.on);
              return (
                <TerminalPane
                  key={entry.id}
                  summary={entry}
                  ceiling={ceiling}
                  liveness={liveness}
                  speaking={speaking.has(entry.id)}
                  bus={bus}
                  visible
                  focused={entry.id === visible}
                  onFocus={() => setHere(entry.id)}
                  onSubmit={(line) => submitLine(entry.id, line)}
                  onClose={() => {
                    void closeTerminal(entry.id)
                      .then(() => again())
                      .catch((error: unknown) => setTrouble(String(error)));
                  }}
                  onPress={(bytes) => {
                    void pressKeys(entry.id, bytes).catch((error: unknown) => setTrouble(String(error)));
                  }}
                  onResize={(cols, rows) => {
                    void resizeTerminal(entry.id, cols, rows).catch((error: unknown) =>
                      setTrouble(String(error)),
                    );
                  }}
                />
              );
            })}
          </div>

          {visible !== null && watched !== null && (
            <div className="terminals__foot">
              {/* WHAT THE AGENT CHANGED, READ WITHOUT LEAVING: the working tree
                  of the workspace this terminal was opened in, as git says it. */}
              <button type="button" onClick={() => setReading((on) => !on)}>
                {reading ? "hide what changed" : `what changed in ${watched.workspaceName}`}
              </button>
              <button
                type="button"
                onClick={() => {
                  void closeTerminal(visible)
                    .then(() => again())
                    .catch((error: unknown) => setTrouble(String(error)));
                }}
              >
                Close this terminal
              </button>
            </div>
          )}

          {reading && watched !== null && (
            <ChangesScreen key={watched.workspaceRoot} root={watched.workspaceRoot} name={watched.workspaceName} />
          )}
        </>
      )}
    </div>
  );
}
