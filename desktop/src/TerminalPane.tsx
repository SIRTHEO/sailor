// Un terminale, disegnato.
//
// **L'EMULATORE NON È NOSTRO, ED È UNA SCELTA SCRITTA.** Interpretare le
// sequenze ANSI a mano è un progetto a sé: il posizionamento del cursore, lo
// schermo alternativo, i colori a 256, i caratteri larghi il doppio, le
// sequenze spezzate a metà fra due letture. `@xterm/xterm` fa quel pezzo, lo fa
// da dieci anni ed è l'emulatore dentro VS Code. La direzione di prodotto 3 di
// questo repo dice esattamente questo: se esiste un progetto vivo che fa quel
// pezzo, si collega.
//
// **QUESTO FILE NON DECIDE NIENTE.** Dove va un tasto, come si legge un byte e
// se un terminale è vivo stanno in `terminal.ts`, in funzioni pure. Qui c'è
// solo il collegamento fra quelle decisioni e ciò che si vede — ed è
// deliberato: un componente che monta un emulatore si prova solo disegnandolo,
// e ciò che si può provare senza disegnare non deve stare dentro.

import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as Emulator } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  keyStroke,
  livenessWord,
  type KeyMode,
  type Liveness,
  type OutputBus,
  type Submitted,
  type TerminalSummary,
} from "./terminal";

/**
 * Quanto grande nasce un terminale prima che qualcuno lo misuri.
 *
 * Non è una preferenza: `terminal_open` vuole `cols` e `rows`, e la misura vera
 * arriva dopo il primo disegno. Ottanta per ventiquattro è la dimensione che
 * ogni programma a schermo intero sa gestire.
 */
export const BORN_COLS = 80;
export const BORN_ROWS = 24;

interface PaneProps {
  summary: TerminalSummary;
  liveness: Liveness;
  /** Dove arrivano i byte del processo: il pannello si iscrive per il proprio `id`. */
  bus: OutputBus;
  /** Nascosto quando è aperta un'altra scheda: l'emulatore resta vivo e continua a ricevere. */
  visible: boolean;
  /** La riga confermata con Invio. Torna dove è finita, e il pannello lo scrive. */
  onSubmit: (line: string) => Promise<Submitted>;
  onPress: (bytes: Uint8Array) => void;
  onResize: (cols: number, rows: number) => void;
}

export function TerminalPane({
  summary,
  liveness,
  bus,
  visible,
  onSubmit,
  onPress,
  onResize,
}: PaneProps) {
  const host = useRef<HTMLDivElement | null>(null);
  const emulator = useRef<Emulator | null>(null);
  const fitter = useRef<FitAddon | null>(null);
  /** La riga in composizione vive in un `ref`: la tastiera arriva fuori da React. */
  const draft = useRef("");
  const [shown, setShown] = useState("");
  // UN TERMINALE NASCE TERMINALE. Il modo predefinito è `raw` perché comporre
  // la riga costa il Tab e i programmi a schermo intero: vedi il prezzo scritto
  // per esteso sopra `keyStroke`.
  const [mode, setMode] = useState<KeyMode>("raw");
  const [routed, setRouted] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);

  // I gestori cambiano a ogni disegno, l'emulatore si monta una volta sola: se
  // entrassero nelle dipendenze del montaggio, ogni render butterebbe via il
  // terminale e con lui tutto ciò che ci è uscito dentro.
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
            // UN TASTO CHE NON PARTE LO DICE. Silenzio e «non ha funzionato»
            // si assomigliano troppo, e sul secondo si preme di nuovo.
            setRefused(action.why);
            break;
        }
      }
    });

    // Il riquadro cambia quando cambia la finestra, non quando React lo decide.
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

  // I byte del processo vanno all'emulatore e non allo stato di React: un
  // `setState` per pezzo ridisegnerebbe la finestra a ogni riga di un `cargo
  // build`. Sono `Uint8Array` fino all'ultimo passaggio, così un accento
  // spezzato fra due eventi si rimette insieme qui dentro invece di perdersi.
  useEffect(() => bus.subscribe(summary.id, (bytes) => emulator.current?.write(bytes)), [bus, summary.id]);

  // Un pannello nascosto non ha larghezza: quando torna visibile va rimisurato,
  // altrimenti resta della dimensione che aveva quando è nato.
  useEffect(() => {
    if (!visible) return;
    const term = emulator.current;
    const fit = fitter.current;
    if (term && fit) refit(fit, term, latest.current.onResize);
  }, [visible]);

  const dead = liveness.state === "closed";

  return (
    <section className="pane" hidden={!visible}>
      <header className="pane__head">
        <span className="label">{summary.workspaceName}</span>
        <span className="pane__root">{summary.workspaceRoot}</span>
        {/* La parola sta accanto alla tinta: divieto 5. */}
        <span className="pane__state" data-state={liveness.state}>
          {livenessWord(liveness)}
        </span>
        {liveness.state === "unknown" && <span className="pane__why">{liveness.why}</span>}
        {liveness.state === "closed" && liveness.status !== null && (
          <span className="pane__why">{liveness.status}</span>
        )}
        <span className="pane__id">{summary.id}</span>
        {/* LO STATO E IL GESTO SONO DUE COSE. Una sola scritta su un pulsante
            non dice se sta nominando com'è messo adesso o cosa succede a
            premerlo, e il modo della tastiera è la cosa che qui si sbaglia
            più caro. */}
        <span className="pane__keys">
          {mode === "compose" ? "la riga la tiene la finestra" : "i tasti vanno diritti al processo"}
        </span>
        <button
          type="button"
          className="pane__mode"
          data-mode={mode}
          onClick={() => setMode(mode === "compose" ? "raw" : "compose")}
          disabled={dead}
        >
          {mode === "compose" ? "torna ai tasti diretti" : "componi una riga da smistare"}
        </button>
      </header>

      {/* L'EMULATORE VIVE QUI DENTRO, E RESTA MONTATO ANCHE DA MORTO: ciò che
          il processo ha scritto prima di finire è la parte che si va a
          rileggere, e smontarlo la cancellerebbe. */}
      <div className="pane__screen" ref={host} />

      <footer className="pane__foot">
        {mode === "compose" ? (
          <span className="pane__draft" data-empty={shown === "" || undefined}>
            {shown === "" ? "scrivi una riga, Invio la manda allo smistamento" : shown}
          </span>
        ) : (
          <span className="pane__draft" data-empty>
            ogni tasto va al processo così com'è, Invio compreso
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
 * Dove è finita la riga, detto a chi guarda.
 *
 * **UNA RIGA DIROTTATA SI VEDE, E SI VEDE QUALE REGOLA L'HA DIROTTATA.** Un
 * terminale che ogni tanto non esegue quello che scrivi è peggio di uno che non
 * smista affatto: diventa imprevedibile, e l'imprevedibilità si paga su ogni
 * riga che si scriverà dopo. Il nome della regola serve a risalire alla riga di
 * JSON che ha deciso, non solo al flusso.
 */
export function routingNote(line: string, answer: Submitted): string {
  if (answer.kind === "command") return `«${line}» è andata alla shell`;
  return `«${line}» non è stata eseguita: la regola «${answer.rule}» l'ha mandata al flusso «${answer.flow}» come «${answer.text}»`;
}

/** Rimisura, e dice al motore la nuova taglia solo se è cambiata davvero. */
function refit(fit: FitAddon, term: Emulator, tell: (cols: number, rows: number) => void): void {
  const before = { cols: term.cols, rows: term.rows };
  try {
    fit.fit();
  } catch {
    // Un riquadro largo zero — un pannello nascosto, la finestra ridotta a
    // niente — non è un guasto: è una misura che non si può prendere adesso.
    return;
  }
  if (term.cols !== before.cols || term.rows !== before.rows) tell(term.cols, term.rows);
}

/** Il valore di un ruolo del foglio, letto dal documento. */
function readToken(name: string): string {
  if (typeof getComputedStyle !== "function") return "";
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/**
 * I colori dell'emulatore vengono dai ruoli, non da una tavolozza sua.
 *
 * **PERCHÉ NON SI LASCIA IL TEMA PREDEFINITO DI XTERM.** È nero su nero-quasi,
 * cioè un riquadro scuro incollato dentro una finestra di carta calda: il
 * divieto 4 riserva il colore allo stato della macchina, e un pannello con una
 * tinta sua è esattamente ciò che vieta. Se un ruolo non si legge — succede
 * fuori da un browser vero — non si inventa niente e l'emulatore tiene il suo:
 * un colore indovinato passerebbe il controllo del foglio senza essere quello
 * che il foglio dice.
 */
function themeFromTokens(): { background?: string; foreground?: string; cursor?: string } {
  const background = readToken("--paper");
  const foreground = readToken("--ink");
  if (background === "" || foreground === "") return {};
  return { background, foreground, cursor: foreground };
}
