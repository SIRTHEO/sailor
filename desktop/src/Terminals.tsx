// I terminali: le schede, i pannelli, e chi apre il prossimo.
//
// **L'ELENCO È DEL MOTORE, NON DI QUESTA SCHERMATA.** `terminal_list` è l'unica
// risposta alla domanda «quali terminali ci sono», e questo componente non ne
// tiene una copia. La ragione non è di stile: un terminale sopravvive alla
// finestra: chi la chiude non uccide la sessione dentro, e al riavvio è
// l'elenco a ritrovarla. Una lista tenuta qui direbbe «nessuno» a una macchina
// che ne ha tre — con la stessa faccia con cui lo direbbe a una macchina che
// non ne ha nessuno.
//
// **QUELLO CHE QUESTA SCHERMATA TIENE DAVVERO SUO** sono tre cose che il motore
// non può sapere: quale scheda guardi, quali terminali hanno mandato
// `terminal_closed` da quando sei qui, e se il canale degli eventi si è
// attaccato. Le prime due sono fatti della finestra; la terza è la ragione per
// cui esiste uno stato «non lo so più».

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

/** Ogni quanto si richiede l'elenco. Un terminale nasce e muore a mano: non serve di più. */
const REFRESH_MS = 4000;

interface TerminalsProps {
  /** Vero dentro il guscio nativo: fuori non c'è nessuno pseudo-terminale da aprire. */
  native: boolean;
}

export function Terminals({ native }: TerminalsProps) {
  const outside = "fuori dal guscio: gli pseudo-terminali li apre il motore";
  const { asked, again } = useAsk<TerminalSummary[]>(native, listTerminals, REFRESH_MS, outside);

  const [here, setHere] = useState<string | null>(null);
  /** Chi ha mandato `terminal_closed`, e con quale esito. */
  const [closed, setClosed] = useState<Map<string, string>>(() => new Map());
  /** Il canale degli eventi: attaccato, o il motivo per cui no. */
  const [channel, setChannel] = useState<{ on: boolean; why: string | null }>({ on: false, why: null });
  const [opening, setOpening] = useState(false);
  const [root, setRoot] = useState("");
  const [program, setProgram] = useState("");
  const [trouble, setTrouble] = useState<string | null>(null);
  /** Byte arrivati per un terminale senza pannello: sarebbero uscita persa. */
  const [orphans, setOrphans] = useState(0);

  const bus = useMemo(() => new OutputBus(), []);
  // `again` cambia identità a ogni disegno; l'ascolto si attacca una volta.
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
        // L'elenco porta anche `alive`: si richiede subito, così le due fonti
        // tornano a dire la stessa cosa invece di divergere fino al prossimo giro.
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
      // L'errore del motore è il testo che `PtyError` produce, e va letto: una
      // cartella che non c'è e una shell che non parte si riparano in due modi
      // diversi, e un `console.error` non li distingue per nessuno.
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
            placeholder="/Users/you/code/sailor"
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
