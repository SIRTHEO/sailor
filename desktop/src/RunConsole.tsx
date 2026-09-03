import { useEffect, useMemo, useRef, useState } from "react";
import type { RunEvent, RunSnapshot } from "./engine";
import { tryT } from "./i18n";
import { totalsArePartial, type RunUsage } from "./flow";

/**
 * A run as it goes: what is running, what finished, what it said.
 *
 * **STEP STATE STREAMS. THE TEXT A STEP PRODUCES DOES NOT.** The shell
 * announces each step opening and closing the instant the store makes it
 * durable, so a second step is seen starting while the run is half done. But
 * `drain_and_wait` in `crates/actions/src/lib.rs` reads stdout with
 * `read_to_end` on a thread of its own, and that buffer only becomes readable
 * at the `join` — an agent that talks for half an hour delivers all of it at
 * once, at the end.
 *
 * So an output line carries **the instant the step closed**, which is when it
 * really arrived, and every box says so while the step is still running.
 * Spreading them over an invented time to look alive is the exact lie this
 * guards against. For real streaming, `read_to_end` becomes `BufReader::lines()`
 * pushing into a channel: that is the only place, everything above it is ready.
 *
 * The front starts together — the cap is `AT_ONCE` in
 * `crates/flow/src/executor.rs`, four steps per wave.
 */

/** Come si guarda una corsa. */
export type ConsoleMode = "inline" | "split";

/** Una riga della vista, con la sua provenienza. */
export interface ConsoleLine {
  key: string;
  at: number;
  stepId: string | null;
  /** Da dove viene: il guscio, l'uscita del passo, i suoi errori, il suo detto. */
  stream: "system" | "stdout" | "stderr" | "said";
  text: string;
}

/**
 * Il tetto di righe tenute in vista. Un agente che parla per mezz'ora può
 * consegnare decine di migliaia di righe in un colpo solo: disegnarle tutte
 * blocca la finestra proprio mentre chi guarda vuole leggere il finale.
 * Si tagliano le **più vecchie**, e il taglio si dichiara invece di far
 * sparire testo in silenzio.
 */
const MAX_LINES = 4000;

function splitText(text: string): string[] {
  return text.replace(/\n+$/, "").split("\n");
}

function pushText(
  lines: ConsoleLine[],
  seq: number,
  at: number,
  stepId: string | null,
  stream: ConsoleLine["stream"],
  text: unknown,
) {
  if (typeof text !== "string" || text.trim() === "") return;
  splitText(text).forEach((row, index) => {
    lines.push({ key: `${seq}:${stream}:${index}`, at, stepId, stream, text: row });
  });
}

/**
 * Da fatti a righe.
 *
 * Sta fuori dal componente perché è la sola parte con una risposta giusta e
 * una sbagliata: una prova può darle degli eventi e guardare cosa produce,
 * senza montare React.
 */
export function linesFromEvents(events: RunEvent[]): ConsoleLine[] {
  const lines: ConsoleLine[] = [];

  for (const event of events) {
    const payload = event.payload as Record<string, unknown> | null;
    switch (event.kind) {
      case "step_started": {
        lines.push({
          key: `${event.seq}:head`,
          at: event.at,
          stepId: event.step_id,
          stream: "system",
          text: `— «${event.step_id}» started`,
        });
        break;
      }
      case "step_closed": {
        const outcome = typeof payload?.outcome === "string" ? payload.outcome : "?";
        const failure = typeof payload?.failure_class === "string" ? payload.failure_class : null;
        lines.push({
          key: `${event.seq}:foot`,
          at: event.at,
          stepId: event.step_id,
          stream: failure ? "stderr" : "system",
          text: `— «${event.step_id}» closed: ${OUTCOME_LABEL[outcome] ?? outcome}${
            failure ? ` — ${whyFailed(failure)}` : ""
          }`,
        });
        // L'uscita di un motore esterno vive dentro `output`; una verifica di
        // shell non conserva testo, e per quella non c'è niente da mostrare
        // oltre all'esito — cosa che il riquadro dichiara.
        const output = payload?.output as Record<string, unknown> | null | undefined;
        if (output && typeof output === "object") {
          pushText(lines, event.seq, event.at, event.step_id, "stdout", output.stdout);
          pushText(lines, event.seq, event.at, event.step_id, "stderr", output.stderr);
          // UN PASSO CHE DICHIARA LA FORMA DELLA RISPOSTA NON DÀ PIÙ `stdout`:
          // la sua uscita è `{status, answer}`, con dentro solo i campi che la
          // forma dichiara. Leggere il solo `stdout` lascerebbe vuoto proprio il
          // riquadro dei passi che rispondono in forma — cioè quelli su cui si
          // conta di più. Un innesco arriva qui come un oggetto anche lui
          // (`{text, who, where, source, kind}`), e si legge nello stesso modo.
          if (output.answer !== undefined && output.answer !== null) {
            pushText(
              lines,
              event.seq,
              event.at,
              event.step_id,
              "stdout",
              typeof output.answer === "string"
                ? output.answer
                : JSON.stringify(output.answer, null, 2),
            );
          }
        }
        // UN PASSO ROTTO NON HA `output`: il testo utile sta tutto in `said`, e
        // il motivo in `failure_class`. È il caso in cui chi guarda ha più
        // bisogno di leggere, e prima di questa riga era l'unico in cui la
        // vista non mostrava niente.
        pushText(lines, event.seq, event.at, event.step_id, "said", payload?.said);
        break;
      }
      // WHAT A STEP SAYS WHILE IT RUNS. It arrives in pieces as the engine
      // writes them, under the pipe it came from: an error mixed into ordinary
      // output and indistinguishable from it is no more visible than silence.
      case "step_text": {
        const pipe = payload?.pipe === "err" ? "stderr" : "stdout";
        pushText(lines, event.seq, event.at, event.step_id, pipe, payload?.text);
        break;
      }
      case "stop_requested": {
        lines.push({
          key: `${event.seq}:stop`,
          at: event.at,
          stepId: null,
          stream: "system",
          text: "══ stop requested: no further step starts; the one running finishes",
        });
        break;
      }
      case "run_ended": {
        const status = typeof payload?.status === "string" ? payload.status : "?";
        const error = typeof payload?.error === "string" ? payload.error : null;
        lines.push({
          key: `${event.seq}:end`,
          at: event.at,
          stepId: null,
          stream: error ? "stderr" : "system",
          text: `══ the run ended: ${status}${error ? ` — ${error}` : ""}`,
        });
        // A run resumed through the window ends with the engine's own report:
        // what was reconciled, what was left to a person, the status it wrote.
        if (typeof payload?.report === "string" && payload.report !== "") {
          lines.push({ key: `${event.seq}:report`, at: event.at, stepId: null, stream: "system", text: payload.report });
        }
        break;
      }
      default: {
        pushText(lines, event.seq, event.at, event.step_id, "system", payload?.text);
      }
    }
  }

  if (lines.length <= MAX_LINES) return lines;
  const kept = lines.slice(lines.length - MAX_LINES);
  kept.unshift({
    key: "trimmed",
    at: kept[0]?.at ?? 0,
    stepId: null,
    stream: "system",
    text: `══ ${lines.length - MAX_LINES} older lines are not shown`,
  });
  return kept;
}

/** Lo stato di un passo per la vista affiancata. */
interface StepPane {
  stepId: string;
  startedAt: number;
  endedAt: number | null;
  outcome: string | null;
  failure: string | null;
  lines: ConsoleLine[];
  /** Vero se il passo ha prodotto del testo suo, oltre alle righe di sistema. */
  spoke: boolean;
  /** L'azione del passo, per dire se conserva testo o solo un esito. */
  action: string | null;
  /**
   * Cosa è entrato nel passo.
   *
   * Arrivava già dentro `step_started` e veniva letto **solo** per indovinare
   * l'azione, poi buttato. Chi guardava una corsa vedeva cosa ogni passo aveva
   * detto e mai cosa gli era stato dato: metà del vincolo «chiarezza per chi
   * guarda» mancava, ed era la metà che spiega l'altra.
   */
  input: unknown;
  /** What came out, kept whole: the lines made from it are not the thing. */
  output: unknown;
}

export function panesFromEvents(events: RunEvent[]): StepPane[] {
  const panes = new Map<string, StepPane>();
  const lines = linesFromEvents(events);

  for (const event of events) {
    if (!event.step_id) continue;
    const payload = event.payload as Record<string, unknown> | null;
    if (event.kind === "step_started") {
      panes.set(event.step_id, {
        stepId: event.step_id,
        startedAt: event.at,
        endedAt: null,
        outcome: null,
        failure: null,
        lines: [],
        spoke: false,
        // Il record del passo porta l'input, da cui si legge cosa esegue.
        action: readAction(payload),
        input: payload?.input ?? null,
        output: null,
      });
    } else if (event.kind === "step_closed") {
      const pane = panes.get(event.step_id);
      if (pane) {
        pane.endedAt = event.at;
        pane.outcome = typeof payload?.outcome === "string" ? payload.outcome : null;
        pane.failure = typeof payload?.failure_class === "string" ? payload.failure_class : null;
        pane.output = payload?.output ?? null;
      }
    }
  }

  for (const line of lines) {
    if (!line.stepId) continue;
    const pane = panes.get(line.stepId);
    if (!pane) continue;
    pane.lines.push(line);
    if (line.stream !== "system") pane.spoke = true;
  }

  return Array.from(panes.values());
}

/**
 * Cosa esegue un passo, letto dal suo record. Un `command` è una verifica di
 * shell — che non conserva testo — un `bin` è un motore esterno, che lo
 * conserva. Serve a dire a chi guarda perché un riquadro resta senza righe.
 */
function readAction(payload: Record<string, unknown> | null): string | null {
  const input = payload?.input;
  if (!input || typeof input !== "object") return null;
  const record = input as Record<string, unknown>;
  // `source` è il campo che solo un innesco dichiara; `tool` ha preso il posto
  // di `bin` quando gli strumenti sono diventati identificativi invece di
  // percorsi di binari, e `bin` resta letto per i flussi scritti prima.
  if (typeof record.source === "string") return "trigger";
  if (typeof record.tool === "string" || typeof record.bin === "string") return "external_engine";
  if (typeof record.command === "string") return "shell_check";
  return null;
}

/**
 * A failure class, in one readable line, from `run.failure.*`.
 *
 * **THE CLASSES ARE STABLE ENGINE NAMES, not free text**: they say *why* a step
 * fell without making anyone read the wall of output it produced. A class the
 * catalogue has never heard of shows as it came — an unknown name is
 * information, an invented sentence is not, which is why this asks `tryT`.
 */
export function whyFailed(failure: string): string {
  return tryT(`run.failure.${failure}`) ?? failure;
}

/** How a step ended, in a word a person reads. */
export const OUTCOME_LABEL: Record<string, string> = {
  Went: "went",
  Broke: "broke",
  Waiting: "waiting",
  Stopped: "stopped",
  Skipped: "skipped",
};

function clock(at: number, since: number): string {
  const delta = Math.max(0, at - since);
  const minutes = Math.floor(delta / 60);
  const seconds = delta % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

interface RunConsoleProps {
  run: RunSnapshot;
  runs: RunSnapshot[];
  mode: ConsoleMode;
  /** Il secondo di adesso, per far salire i contatori dei passi aperti. */
  now: number;
  /**
   * Perché la vista non è in ascolto, quando non lo è. Una vista che si
   * aggiorna interrogando invece che in ascolto resta vera, ma con un ritardo:
   * chi guarda deve saperlo, perché è la differenza fra «non è ancora successo»
   * e «non l'ho ancora chiesto».
   */
  listenFailure: string | null;
  /**
   * Quanto è costata questa corsa, quando il deposito lo sa. `null` mentre non
   * lo sa ancora — e in quel caso non si mostra niente, invece di mostrare zero.
   */
  usage: RunUsage | null;
  onMode: (mode: ConsoleMode) => void;
  onPick: (runId: string) => void;
  onClose: () => void;
  /** Asks the engine to stop this run before its next step. Rejects with the reason. */
  onStop: () => Promise<void>;
}

/** Whether a stop has been asked and the run has not ended yet. */
export function stopRequested(run: RunSnapshot): boolean {
  return run.status === "running" && run.events.some((event) => event.kind === "stop_requested");
}

/** Micro-units of currency as a person reads them: 128_541 → «$0.1285». */
function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(4)}`;
}

/** Thousands separated, the English way. */
function tokens(count: number): string {
  return count.toLocaleString("en-GB");
}

/**
 * La riga della spesa.
 *
 * **LA CACHE SCRITTA STA IN CHIARO, SEPARATA DA QUELLA LETTA.** Sono l'opposto
 * l'una dell'altra: leggere costa una frazione dell'ingresso, scrivere costa
 * più dell'ingresso. Su una chiamata misurata il 30/08/2026 la sola scrittura
 * era il 96% della spesa, con due token d'ingresso: metterle nella stessa
 * casella nasconderebbe l'unica voce che conta davvero.
 *
 * **E UN TOTALE PARZIALE LO DICE.** Se qualche chiamata non ha dichiarato i
 * propri conteggi, o non aveva un prezzo, la cifra qui sotto è più bassa del
 * vero: tacerlo sarebbe presentare una somma che nasconde ciò che le manca.
 */
function Spend({ usage }: { usage: RunUsage }) {
  const t = usage.tokens;
  if (t.calls === 0) return null;
  return (
    <div className="console__spend">
      <span className="console__spend-cost">{money(usage.total_cost_micros)}</span>
      <span>
        {t.calls} {t.calls === 1 ? "call" : "calls"}
      </span>
      <span>↑ {tokens(t.input_tokens)}</span>
      <span>↓ {tokens(t.output_tokens)}</span>
      {t.cached_tokens > 0 && <span title="read from the cache">cache read {tokens(t.cached_tokens)}</span>}
      {t.cache_write_tokens > 0 && (
        <span title="written to the cache: dearer than ordinary input">
          cache written {tokens(t.cache_write_tokens)}
        </span>
      )}
      {t.total_tokens_only > 0 && (
        <span title="engines that declare only the total, without the two sides">
          unsplit total {tokens(t.total_tokens_only)}
        </span>
      )}
      {totalsArePartial(t) && (
        <span className="console__spend-partial">
          partial total:{" "}
          {t.calls_without_tokens > 0 && `${t.calls_without_tokens} without counts`}
          {t.calls_without_tokens > 0 && t.calls_without_cost > 0 && ", "}
          {t.calls_without_cost > 0 && `${t.calls_without_cost} without a price`}
        </span>
      )}
    </div>
  );
}

export function RunConsole({
  run,
  runs,
  mode,
  now,
  listenFailure,
  usage,
  onMode,
  onPick,
  onClose,
  onStop,
}: RunConsoleProps) {
  const [stopTrouble, setStopTrouble] = useState<string | null>(null);
  const lines = useMemo(() => linesFromEvents(run.events), [run.events]);
  const panes = useMemo(() => panesFromEvents(run.events), [run.events]);
  const tail = useRef<HTMLDivElement | null>(null);

  // La coda resta in vista mentre la corsa avanza. Non si scorre se chi guarda
  // si è spostato a leggere più in su: strappargli la vista di sotto è il modo
  // di rendere illeggibile proprio la riga che stava cercando.
  useEffect(() => {
    const box = tail.current;
    if (!box) return;
    const nearBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 80;
    if (nearBottom) box.scrollTop = box.scrollHeight;
  }, [lines.length, mode]);

  const running = run.status === "running";
  const stopping = stopRequested(run);
  const openPanes = panes.filter((pane) => pane.endedAt === null);

  return (
    <section className="console" aria-label="the run, as it goes">
      <header className="console__bar">
        <span className="console__title">Run</span>

        <select
          className="console__pick"
          value={run.run_id}
          aria-label="which run to watch"
          onChange={(event) => onPick(event.target.value)}
        >
          {runs.map((entry) => (
            <option key={entry.run_id} value={entry.run_id}>
              {entry.flow} · {entry.status}
            </option>
          ))}
        </select>

        <span className="console__status" data-status={run.status}>
          {stopping
            ? `stopping after the current step · ${clock(now, run.started_at)}`
            : running
              ? `running for ${clock(now, run.started_at)}`
              : run.status}
        </span>

        {/* THE STOP IS HONEST ABOUT WHAT IT CAN DO: the next step does not
            start; the one at work finishes, because the engine cannot take a
            step back from an agent already working on it. */}
        {running && !stopping && (
          <button
            type="button"
            className="console__stop"
            title="no further step starts; the one running finishes"
            onClick={() => {
              onStop().catch((error: unknown) => setStopTrouble(String(error)));
            }}
          >
            ■ Stop
          </button>
        )}
        {stopTrouble && <span className="console__trouble">{stopTrouble}</span>}

        <div className="console__spacer" />

        {/* The two ways of looking, at the reader's choice. */}
        <div className="console__modes" role="group" aria-label="how to look">
          <button type="button" data-on={mode === "inline" || undefined} onClick={() => onMode("inline")}>
            inline
          </button>
          <button type="button" data-on={mode === "split" || undefined} onClick={() => onMode("split")}>
            side by side
          </button>
        </div>

        <button type="button" className="console__close" onClick={onClose} title="close the view">
          ✕
        </button>
      </header>

      {/* LA FRASE CHE IMPEDISCE DI CREDERE A UNA COSA FALSA. Lo stato dei passi
          arriva mentre accade; il testo che un passo produce arriva tutto alla
          sua chiusura, perché il motore lo legge fino in fondo prima di
          consegnarlo. Chi guarda deve saperlo mentre guarda, non dopo. */}
      <div className="console__truth">
        steps are seen opening and closing as it happens; the text a step produces arrives all
        at once when it closes — the engine reads it to the end before handing it over
      </div>

      {listenFailure && <div className="console__truth">{listenFailure}</div>}

      {usage && <Spend usage={usage} />}

      {mode === "inline" ? (
        <div className="console__lines" ref={tail}>
          {lines.length === 0 && <div className="console__empty">no lines, yet</div>}
          {lines.map((line) => (
            <div className="console__line" key={line.key} data-stream={line.stream}>
              <span className="console__time">{clock(line.at, run.started_at)}</span>
              {/* Chi ha prodotto la riga: nella vista in linea è l'unica cosa
                  che distingue due passi mescolati. */}
              <span className="console__who">{line.stepId ?? "run"}</span>
              <span className="console__text">{line.text}</span>
            </div>
          ))}
          {running && openPanes.length > 0 && (
            <div className="console__waiting">
              {openPanes
                .map((pane) => `«${pane.stepId}» running for ${clock(now, pane.startedAt)}`)
                .join(" · ")}
            </div>
          )}
        </div>
      ) : (
        <div className="console__panes" ref={tail}>
          {panes.length === 0 && <div className="console__empty">no steps, yet</div>}
          {panes.map((pane) => (
            <article className="pane" key={pane.stepId} data-open={pane.endedAt === null || undefined}>
              <header className="pane__bar">
                <span className="pane__id">{pane.stepId}</span>
                <span className="pane__state" data-outcome={pane.outcome ?? "open"}>
                  {pane.endedAt === null
                    ? `running · ${clock(now, pane.startedAt)}`
                    : `${OUTCOME_LABEL[pane.outcome ?? ""] ?? pane.outcome ?? "?"} · ${clock(
                        pane.endedAt,
                        pane.startedAt,
                      )}`}
                </span>
              </header>
              <div className="pane__body">
                {/* COSA È ENTRATO, prima di cosa è uscito: è l'ordine in cui si
                    capisce un passo, e finora c'era solo la seconda metà.
                    Chiuso di suo — un input lungo seppellirebbe il testo del
                    passo, che resta la cosa che si guarda per prima. */}
                {pane.input !== null && pane.input !== undefined && (
                  <details className="pane__input">
                    <summary className="pane__input-head">what came in</summary>
                    <pre className="pane__code">{JSON.stringify(pane.input, null, 2)}</pre>
                  </details>
                )}
                {/* «Righe» non è «testo del passo»: le righe di sistema —
                    partito, ha chiuso — ci sono sempre, e contarle come testo
                    farebbe sparire la nota proprio nei riquadri che ne hanno
                    bisogno, cioè quelli dove il passo non ha detto niente. */}
                {/* A running step that has said nothing has said nothing yet:
                    its text arrives as the engine writes it, not at the end. */}
                {!pane.spoke && pane.endedAt === null && (
                  <div className="pane__note">running, and it has not said anything yet</div>
                )}
                {!pane.spoke && pane.endedAt !== null && (
                  <div className="pane__note">
                    {pane.action === "shell_check"
                      ? "a shell check keeps no text of its own: of this step the outcome remains"
                      : "this step produced no text"}
                  </div>
                )}
                {pane.lines.map((line) => (
                  <div className="pane__line" key={line.key} data-stream={line.stream}>
                    {line.text}
                  </div>
                ))}
                {pane.failure && <div className="pane__failure">{whyFailed(pane.failure)}</div>}
              </div>
            </article>
          ))}
        </div>
      )}

      {/* QUI C'ERA UN AVVISO, E NON C'È PIÙ PERCHÉ È DIVENTATO FALSO. Diceva
          che due riquadri affiancati mostravano due lavori di cui uno solo
          avanzava davvero, perché il motore percorreva i passi in fila. Dal
          30/08/2026 il fronte parte insieme (misurato: due passi da sei secondi
          in 6,07), quindi due riquadri che avanzano insieme adesso dicono la
          verità. Un avviso che resta dopo che il difetto è andato via insegna a
          non leggere gli avvisi. */}
    </section>
  );
}
