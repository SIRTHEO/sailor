// The terminals: the tabs, the panes, and who opens the next one.

// **THE LIST BELONGS TO THE ENGINE, NOT TO THIS SCREEN.** `terminal_list` is
// the only answer to "which terminals exist", and this component keeps no copy.
// A terminal outlives the window — closing it does not kill the session inside
// — so a list kept here would say "none" to a machine that has three, wearing
// the same face it would wear on a machine that has none.

// **WHAT THIS SCREEN REALLY OWNS** are three things the engine cannot know:
// which tab you are watching, which terminals have sent `terminal_closed` since
// you arrived, and whether the event channel attached. The third is why a
// "no longer known" state exists at all.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAsk } from "./ask";
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
  submitLine,
  watchTerminals,
  type TerminalSummary,
} from "./terminal";

/** How often the list is re-asked. A terminal is born and dies by hand. */
const REFRESH_MS = 4000;

interface TerminalsProps {
  /** True inside the native shell: outside there is no pty to open. */
  native: boolean;
}

/**
 * The placeholder of the "workspace" field. **IT NAMES NO REAL MACHINE**: a
 * developer's absolute path in a product field breaks the charter, and a gate
 * finds one. Exported so the test reads it here, not a copy: a text written
 * twice diverges on the first edit — red for a wrong reason, or green over air.
 */
export const WORKSPACE_HINT = "/Users/you/code/sailor";

export function Terminals({ native }: TerminalsProps) {
  const outside = "fuori dal guscio: gli pseudo-terminali li apre il motore";
  const { asked, again } = useAsk<TerminalSummary[]>(native, listTerminals, REFRESH_MS, outside);

  const [here, setHere] = useState<string | null>(null);
  /** Who sent `terminal_closed`, and with what outcome. */
  const [closed, setClosed] = useState<Map<string, string>>(() => new Map());
  /** The event channel: attached, or the reason it is not. */
  const [channel, setChannel] = useState<{ on: boolean; why: string | null }>({ on: false, why: null });
  const [opening, setOpening] = useState(false);
  const [root, setRoot] = useState("");
  const [program, setProgram] = useState("");
  const [trouble, setTrouble] = useState<string | null>(null);
  /** Bytes arrived for a terminal with no pane: they would be lost output. */
  const [orphans, setOrphans] = useState(0);

  const bus = useMemo(() => new OutputBus(), []);
  // `again` changes identity on every render; the listener attaches once.
  const refresh = useRef(again);
  refresh.current = again;

  useEffect(() => {
    if (!native) {
      setChannel({ on: false, why: outside });
      return;
    }
    let stop: (() => void) | null = null;
    let dropped = false;
    void watchTerminals({
      onOutput: (id, bytes) => {
        if (!bus.deliver(id, bytes)) setOrphans((seen) => seen + 1);
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

  const open = useCallback(async () => {
    setTrouble(null);
    setOpening(true);
    try {
      const born = await openTerminal({
        workspaceRoot: root.trim(),
        program: program.trim() === "" ? undefined : program.trim(),
        cols: BORN_COLS,
        rows: BORN_ROWS,
      });
      setHere(born.id);
      again();
    } catch (error) {
      // The engine's error is the text `PtyError` produces, and it has to be
      // read: a missing folder and a shell that will not start are fixed in two
      // different ways, and a `console.error` tells nobody them apart.
      setTrouble(String(error));
    } finally {
      setOpening(false);
    }
  }, [root, program, again]);

  if (asked.state === "mute") {
    return (
      <div className="terminals">
        <p className="terminals__mute">Non riesco a chiedere quali terminali sono aperti: {asked.why}</p>
      </div>
    );
  }

  if (asked.state === "asking") {
    return (
      <div className="terminals">
        <p className="terminals__mute">Chiedo al motore quali terminali sono aperti…</p>
      </div>
    );
  }

  const opened = asked.value;
  const shown = opened.some((entry) => entry.id === here) ? here : (opened[0]?.id ?? null);

  return (
    <div className="terminals">
      {/* LA CARTELLA SI DICHIARA APRENDO. Non esiste un terminale generico a cui
          poi si dice dove andare: lo spazio di lavoro è parte di cosa il
          terminale è, ed è la condizione perché lo smistamento sappia di quale
          progetto si sta parlando. */}
      <form
        className="terminals__open"
        onSubmit={(event) => {
          event.preventDefault();
          void open();
        }}
      >
        <label className="terminals__field">
          <span className="label">Spazio di lavoro</span>
          <input
            className="terminals__input"
            value={root}
            placeholder={WORKSPACE_HINT}
            onChange={(event) => setRoot(event.target.value)}
          />
        </label>
        <label className="terminals__field">
          <span className="label">Cosa avviare</span>
          <input
            className="terminals__input"
            value={program}
            placeholder="la shell di casa"
            onChange={(event) => setProgram(event.target.value)}
          />
        </label>
        <button type="submit" className="is-primary" disabled={opening || root.trim() === ""}>
          {opening ? "apro…" : "Apri un terminale"}
        </button>
      </form>

      {trouble !== null && (
        <p className="terminals__trouble" data-gravity="danger">
          {trouble}
        </p>
      )}

      {channel.why !== null && (
        <p className="terminals__trouble" data-gravity="warn">
          {channel.why} — finché è così, questi pannelli non ricevono né l'uscita né la fine di un processo.
        </p>
      )}

      {orphans > 0 && (
        <p className="terminals__trouble" data-gravity="warn">
          {orphans} pezzi di uscita sono arrivati per un terminale senza pannello, e sono andati persi.
        </p>
      )}

      {opened.length === 0 ? (
        <p className="terminals__empty">Nessun terminale aperto. Non è lo stesso che non poterlo chiedere.</p>
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
                  data-here={entry.id === shown || undefined}
                  data-state={liveness.state}
                  onClick={() => setHere(entry.id)}
                >
                  {entry.workspaceName}
                  {/* La parola porta lo stato quanto la tinta: divieto 5. */}
                  <span className="terminals__word">{livenessWord(liveness)}</span>
                </button>
              );
            })}
          </nav>

          <div className="terminals__panes">
            {opened.map((entry) => {
              const liveness = livenessOf(entry, closed, channel.on);
              return (
                <TerminalPane
                  key={entry.id}
                  summary={entry}
                  liveness={liveness}
                  bus={bus}
                  visible={entry.id === shown}
                  onSubmit={(line) => submitLine(entry.id, line)}
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

          {shown !== null && (
            <div className="terminals__foot">
              <button
                type="button"
                onClick={() => {
                  void closeTerminal(shown)
                    .then(() => again())
                    .catch((error: unknown) => setTrouble(String(error)));
                }}
              >
                Chiudi questo terminale
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
