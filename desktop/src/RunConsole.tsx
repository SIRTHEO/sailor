import { useEffect, useMemo, useRef } from "react";
import type { RunEvent, RunSnapshot } from "./engine";
import { totalsArePartial, type RunUsage } from "./flow";

/**
 * La vista di una corsa: cosa sta girando adesso, cosa ha finito, cosa ha detto.
 *
 * ## COSA ARRIVA MENTRE IL PASSO GIRA, E COSA NO — misurato, non supposto
 *
 * Due cose diverse viaggiano su questo canale, e confonderle è il modo di
 * rendere una vista d'esecuzione peggiore di nessuna vista.
 *
 * **Lo stato dei passi scorre davvero.** Il guscio annuncia l'apertura e la
 * chiusura di ogni passo nell'istante in cui il deposito le rende durevoli.
 * Misura del 28/08/2026 sul flusso di prova: `sinistra` aperto al secondo 0 e
 * chiuso al 6, `destra` aperto al 6 e chiuso al 13. Il secondo passo si è visto
 * partire mentre la corsa era ancora a metà, non alla fine.
 *
 * **Il testo che un passo produce non scorre**, e non dipende da questa
 * finestra: `crates/actions/src/lib.rs` legge lo stdout del processo con
 * `read_to_end` su un thread a parte, e quel buffer diventa leggibile solo al
 * `join`, cioè quando il processo è finito. Non esiste, oggi, nessun punto da
 * cui prendere una riga a metà: un agente che parla per mezz'ora consegna tutto
 * il suo testo in un colpo solo, alla fine.
 *
 * Perciò le righe di uscita portano **l'istante della chiusura del passo**, che
 * è quando sono arrivate davvero, e ogni riquadro lo dichiara mentre il passo
 * sta ancora girando. Spalmarle su un tempo inventato per farle sembrare vive
 * sarebbe la bugia esatta da cui questo commento difende.
 *
 * Perché scorra per davvero va cambiato `drain_and_wait` in
 * `crates/actions/src/lib.rs`: `read_to_end` diventa `BufReader::lines()`, e
 * ogni riga va spinta in un canale che il chiamante possa leggere mentre il
 * processo vive. È l'unico punto: sopra, `flow` e il guscio sono già pronti a
 * far passare i fatti nell'istante in cui accadono.
 *
 * ## E I PASSI IN PARALLELO
 *
 * Dal 30/08/2026 il fronte parte **insieme**: due passi indipendenti da sei
 * secondi impiegano 6,07 secondi in tutto, tre ne impiegano 6,05. Fino a quel
 * giorno giravano in fila — due ne impiegavano dodici — e questa vista lo
 * dichiarava a chi guardava, perché due riquadri affiancati suggeriscono due
 * lavori che avanzano insieme e uno solo si muoveva. Ora la riga non serve più
 * e se n'è andata con la ragione che la teneva.
 *
 * Il tetto è quattro passi per ondata (`AT_ONCE` in
 * `crates/flow/src/executor.rs`): un fronte più largo si esegue a gruppi.
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
          text: `— «${event.step_id}» è partito`,
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
          text: `— «${event.step_id}» ha chiuso: ${OUTCOME_LABEL[outcome] ?? outcome}${
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
      case "run_ended": {
        const status = typeof payload?.status === "string" ? payload.status : "?";
        const error = typeof payload?.error === "string" ? payload.error : null;
        lines.push({
          key: `${event.seq}:end`,
          at: event.at,
          stepId: null,
          stream: error ? "stderr" : "system",
          text: `══ la corsa è finita: ${status}${error ? ` — ${error}` : ""}`,
        });
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
    text: `══ ${lines.length - MAX_LINES} righe più vecchie non sono mostrate`,
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
      });
    } else if (event.kind === "step_closed") {
      const pane = panes.get(event.step_id);
      if (pane) {
        pane.endedAt = event.at;
        pane.outcome = typeof payload?.outcome === "string" ? payload.outcome : null;
        pane.failure = typeof payload?.failure_class === "string" ? payload.failure_class : null;
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
 * Le classi di guasto, in una riga che si legge.
 *
 * **SONO NOMI STABILI DEL MOTORE, non testo libero**: dicono *perché* un passo
 * è caduto senza costringere a leggere il muro di testo che il passo ha
 * prodotto. Una classe che non è in questo elenco si mostra com'è — un nome
 * sconosciuto è un'informazione, una traduzione inventata no.
 */
const FAILURE_LABEL: Record<string, string> = {
  engine_exit_error: "il motore è uscito con un errore",
  engine_timed_out: "il motore ha superato il tempo massimo",
  engine_spawn_failed: "il motore non è partito",
  tool_unavailable: "lo strumento non c'è su questa macchina",
  no_tool_resolver: "nessuno sa dove trovare quello strumento",
  answer_not_json: "la risposta non era JSON",
  answer_off_shape: "la risposta non ha la forma dichiarata",
  shape_not_in_prompt: "la forma pretesa non è stata chiesta al motore",
  check_failed: "la verifica non è passata",
  check_timed_out: "la verifica ha superato il tempo massimo",
  listening_not_built: "questa sorgente di innesco non sa ancora ascoltare",
  unknown_trigger_source: "sorgente di innesco sconosciuta",
  empty_signal: "il segnale è arrivato senza consegna",
  invalid_input: "gli ingressi del passo non sono nella forma attesa",
};

export function whyFailed(failure: string): string {
  return FAILURE_LABEL[failure] ?? failure;
}

/** Come è finito un passo, detto in italiano. */
export const OUTCOME_LABEL: Record<string, string> = {
  Went: "andato",
  Broke: "rotto",
  Waiting: "in attesa",
  Stopped: "fermato",
  Skipped: "saltato",
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
}

/** Micro-unità di valuta come le legge una persona: 128_541 → «0,1285 $». */
function money(micros: number): string {
  return `${(micros / 1_000_000).toFixed(4).replace(".", ",")} $`;
}

/** Migliaia separate, alla maniera italiana. */
function tokens(count: number): string {
  return count.toLocaleString("it-IT");
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
function Spesa({ usage }: { usage: RunUsage }) {
  const t = usage.tokens;
  if (t.calls === 0) return null;
  return (
    <div className="console__spesa">
      <span className="console__spesa-costo">{money(usage.total_cost_micros)}</span>
      <span>
        {t.calls} {t.calls === 1 ? "chiamata" : "chiamate"}
      </span>
      <span>↑ {tokens(t.input_tokens)}</span>
      <span>↓ {tokens(t.output_tokens)}</span>
      {t.cached_tokens > 0 && <span title="letti dalla cache">cache letta {tokens(t.cached_tokens)}</span>}
      {t.cache_write_tokens > 0 && (
        <span title="scritti in cache: costano più dell'ingresso normale">
          cache scritta {tokens(t.cache_write_tokens)}
        </span>
      )}
      {t.total_tokens_only > 0 && (
        <span title="motori che dichiarano solo il totale, senza separare i lati">
          totale non spezzato {tokens(t.total_tokens_only)}
        </span>
      )}
      {totalsArePartial(t) && (
        <span className="console__spesa-parziale">
          totale parziale:{" "}
          {t.calls_without_tokens > 0 && `${t.calls_without_tokens} senza conteggi`}
          {t.calls_without_tokens > 0 && t.calls_without_cost > 0 && ", "}
          {t.calls_without_cost > 0 && `${t.calls_without_cost} senza prezzo`}
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
}: RunConsoleProps) {
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
  const openPanes = panes.filter((pane) => pane.endedAt === null);

  return (
    <section className="console" aria-label="vista dell'esecuzione">
      <header className="console__bar">
        <span className="console__title">Esecuzione</span>

        <select
          className="console__pick"
          value={run.run_id}
          aria-label="quale corsa guardare"
          onChange={(event) => onPick(event.target.value)}
        >
          {runs.map((entry) => (
            <option key={entry.run_id} value={entry.run_id}>
              {entry.flow} · {entry.status}
            </option>
          ))}
        </select>

        <span className="console__status" data-status={run.status}>
          {running ? `in corso da ${clock(now, run.started_at)}` : run.status}
        </span>

        <div className="console__spacer" />

        {/* I due modi di guardare, a scelta di chi guarda. */}
        <div className="console__modes" role="group" aria-label="modo di visualizzazione">
          <button type="button" data-on={mode === "inline" || undefined} onClick={() => onMode("inline")}>
            in linea
          </button>
          <button type="button" data-on={mode === "split" || undefined} onClick={() => onMode("split")}>
            affiancata
          </button>
        </div>

        <button type="button" className="console__close" onClick={onClose} title="chiudi la vista">
          ✕
        </button>
      </header>

      {/* LA FRASE CHE IMPEDISCE DI CREDERE A UNA COSA FALSA. Lo stato dei passi
          arriva mentre accade; il testo che un passo produce arriva tutto alla
          sua chiusura, perché il motore lo legge fino in fondo prima di
          consegnarlo. Chi guarda deve saperlo mentre guarda, non dopo. */}
      <div className="console__truth">
        i passi si vedono aprirsi e chiudersi mentre accade; il testo che un passo produce arriva
        tutto insieme alla sua chiusura — il motore lo legge fino alla fine prima di consegnarlo
      </div>

      {listenFailure && <div className="console__truth">{listenFailure}</div>}

      {usage && <Spesa usage={usage} />}

      {mode === "inline" ? (
        <div className="console__lines" ref={tail}>
          {lines.length === 0 && <div className="console__empty">nessuna riga, ancora</div>}
          {lines.map((line) => (
            <div className="console__line" key={line.key} data-stream={line.stream}>
              <span className="console__time">{clock(line.at, run.started_at)}</span>
              {/* Chi ha prodotto la riga: nella vista in linea è l'unica cosa
                  che distingue due passi mescolati. */}
              <span className="console__who">{line.stepId ?? "corsa"}</span>
              <span className="console__text">{line.text}</span>
            </div>
          ))}
          {running && openPanes.length > 0 && (
            <div className="console__waiting">
              {openPanes
                .map((pane) => `«${pane.stepId}» gira da ${clock(now, pane.startedAt)}`)
                .join(" · ")}{" "}
              — il suo testo comparirà alla chiusura
            </div>
          )}
        </div>
      ) : (
        <div className="console__panes" ref={tail}>
          {panes.length === 0 && <div className="console__empty">nessun passo, ancora</div>}
          {panes.map((pane) => (
            <article className="pane" key={pane.stepId} data-open={pane.endedAt === null || undefined}>
              <header className="pane__bar">
                <span className="pane__id">{pane.stepId}</span>
                <span className="pane__state" data-outcome={pane.outcome ?? "open"}>
                  {pane.endedAt === null
                    ? `in corso · ${clock(now, pane.startedAt)}`
                    : `${OUTCOME_LABEL[pane.outcome ?? ""] ?? pane.outcome ?? "?"} · ${clock(
                        pane.endedAt,
                        pane.startedAt,
                      )}`}
                </span>
              </header>
              <div className="pane__body">
                {/* «Righe» non è «testo del passo»: le righe di sistema —
                    partito, ha chiuso — ci sono sempre, e contarle come testo
                    farebbe sparire la nota proprio nei riquadri che ne hanno
                    bisogno, cioè quelli dove il passo non ha detto niente. */}
                {!pane.spoke && pane.endedAt === null && (
                  <div className="pane__note">gira; il suo testo comparirà alla chiusura</div>
                )}
                {!pane.spoke && pane.endedAt !== null && (
                  <div className="pane__note">
                    {pane.action === "shell_check"
                      ? "una verifica di shell non conserva il proprio testo: di questo passo resta l'esito"
                      : "questo passo non ha prodotto testo"}
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
