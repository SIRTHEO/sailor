// The first screen: what is happening now.
//
// NOT AN INVENTORY OF FLOWS. Opening on the canvas answers «what could I run»,
// while whoever reopens the window is asking whether something is still
// working, something broke, or something is waiting on them.
//
// The question is asked directly: `open_runs` asks the store what is open.
// There is no historical list to filter, because that is not the question — in
// fifteen products compared, every one of them filtered a history by status.
//
// «WAITING ON YOU» IS A STATE, NOT A DOT. It is a named group, at the top, and
// the word sits beside the tint: prohibition 5 does not let a colour carry a
// state on its own.

import { Fragment } from "react";
import { useAsk, useClock } from "./ask";
import { openRuns, todaySummary, type DaySummary, type OpenRun } from "./engine";
import { Handed } from "./Handed";

/** Ogni quanto si richiede l'elenco al deposito. */
const REFRESH_MS = 4000;

/** Il riepilogo di oggi cambia più lentamente: somma corse intere. */
const SUMMARY_MS = 30000;

/** Un numero con i separatori delle migliaia, come lo legge una persona. */
function count(value: number): string {
  return value.toLocaleString("en-GB");
}

/** From micro-units to dollars with three decimals: below a thousandth nothing is decided. */
function money(micros: number): string {
  return `$${(micros / 1_000_000).toFixed(3)}`;
}

/**
 * Il riepilogo di oggi.
 *
 * **DICE ANCHE QUELLO CHE NON HA POTUTO MISURARE.** È la riga che negli altri
 * prodotti manca, e la ricognizione del 31/08/2026 ha trovato perché conta:
 * Langfuse mostrava 4.509 token dove erano 2.265, LangSmith gonfia di 75-200
 * volte con le immagini e non conta la cache dei prompt, e uno di Arize dice di
 * Phoenix che «il costo è calcolato correttamente nel database, ma è difficile
 * capirlo dalla UI». Un numero mostrato con autorità e sbagliato è peggio di un
 * numero assente. Qui, se qualche chiamata non ha portato token o prezzo, la
 * cifra si legge accanto al numero di chiamate che non la compongono.
 */
function Today({ summary }: { summary: DaySummary }) {
  if (!summary.ledger_present) {
    return (
      <p className="now__mute">
        The ledger does not exist yet: nothing has ever run on this machine. No count is known, which is not
        the same as «zero».
      </p>
    );
  }
  const seen = summary.input_tokens + summary.output_tokens + summary.cached_tokens + summary.cache_write_tokens;
  return (
    <section className="today">
      <span className="today__label">Today</span>
      <span className="today__cell">
        <b>{count(summary.runs)}</b> runs
      </span>
      <span className="today__cell">
        <b>{count(summary.went)}</b> went
      </span>
      {summary.broke > 0 && (
        <span className="today__cell" data-gravity="danger">
          <b>{count(summary.broke)}</b> broke
        </span>
      )}
      {summary.still_open > 0 && (
        <span className="today__cell">
          <b>{count(summary.still_open)}</b> still open
        </span>
      )}
      <span className="today__cell">
        <b>{count(seen)}</b> token
      </span>
      <span className="today__cell">
        <b>{money(summary.cost_micros)}</b>
      </span>
      {(summary.unmeasured > 0 || summary.unpriced > 0) && (
        <span className="today__caveat" data-gravity="warn">
          {summary.unmeasured > 0 && `${count(summary.unmeasured)} calls without tokens`}
          {summary.unmeasured > 0 && summary.unpriced > 0 && " · "}
          {summary.unpriced > 0 && `${count(summary.unpriced)} without a price`}
          {" — the figures above do not contain them"}
        </span>
      )}
    </section>
  );
}

/**
 * Da quanto dura, detto come lo direbbe una persona.
 *
 * **SI ARROTONDA PER DIFETTO, SEMPRE.** «2 h» su una corsa ferma da due ore e
 * cinquanta minuti è meno grave di «3 h» su una ferma da due e dieci: chi
 * legge decide se intervenire, e un numero gonfiato lo fa intervenire su una
 * cosa che non è ancora un problema.
 */
export function howLong(seconds: number): string {
  if (seconds < 0) return "—";
  if (seconds < 60) return `${Math.floor(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ${minutes % 60} min`;
  const days = Math.floor(hours / 24);
  return `${days} d ${hours % 24} h`;
}

/**
 * Le corse divise nei due gruppi, ciascuno dalla più vecchia.
 *
 * L'ordine dentro un gruppo arriva già dal motore; qui si separa soltanto, e
 * la separazione è il punto: una corsa che aspetta una persona e una che sta
 * lavorando non si mettono in coda insieme, perché solo una delle due chiede
 * qualcosa a chi guarda.
 */
export function groupRuns(runs: OpenRun[]): { waiting: OpenRun[]; working: OpenRun[] } {
  return {
    waiting: runs.filter((run) => run.state === "waiting"),
    working: runs.filter((run) => run.state === "working"),
  };
}

interface NowProps {
  /** Vero dentro il guscio nativo: fuori non c'è deposito da interrogare. */
  native: boolean;
  /** Aprire la corsa sulla tela, per guardarci dentro. */
  onOpen: (runId: string) => void;
}

export function Now({ native, onOpen }: NowProps) {
  const outside = "outside the shell: the engine reads the ledger";
  const { asked } = useAsk<OpenRun[]>(native, openRuns, REFRESH_MS, outside);
  const { asked: day } = useAsk<DaySummary>(native, todaySummary, SUMMARY_MS, outside);
  const now = useClock();

  if (asked.state === "mute") {
    // UN DEPOSITO MUTO SI DICE. Una schermata vuota e un deposito irraggiungibile
    // si assomigliano troppo, e la seconda è quella in cui si continua a lavorare
    // credendo che non stia girando niente.
    return (
      <div className="now">
        <p className="now__mute">Cannot ask what is running: {asked.why}</p>
      </div>
    );
  }

  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Asking the ledger what is open…</p>
      </div>
    );
  }

  const { waiting, working } = groupRuns(asked.value);

  return (
    <div className="now">
      {day.state === "answered" && <Today summary={day.value} />}
      {asked.value.length === 0 && (
        <p className="now__empty">Nothing is running, and nothing waits for you.</p>
      )}
      {waiting.length > 0 && (
        <RunGroup
          title="Waiting for you"
          note="still until you do something"
          runs={waiting}
          now={now}
          onOpen={onOpen}
        />
      )}
      {working.length > 0 && (
        <RunGroup title="At work" note="somebody or something is working on them" runs={working} now={now} onOpen={onOpen} />
      )}
    </div>
  );
}

/**
 * Un gruppo di corse, disegnato senza chiedere niente a nessuno.
 *
 * **È SEPARATO DA `Now` PERCHÉ SIA MISURABILE.** `Now` interroga il deposito, e
 * fuori dal guscio nativo non ha nessuno a cui chiedere: una prova che
 * disegnasse `Now` misurerebbe la frase «non riesco a chiedere» e crederebbe di
 * aver guardato la schermata. Il controllo del contrasto disegna questo.
 */
interface GroupProps {
  title: string;
  note: string;
  runs: OpenRun[];
  now: number;
  onOpen: (runId: string) => void;
}

export function RunGroup({ title, note, runs, now, onOpen }: GroupProps) {
  return (
    <section className="now__group">
      <header className="now__head">
        <h2 className="now__title">{title}</h2>
        <span className="now__count">{runs.length}</span>
        <span className="now__note">{note}</span>
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>run</th>
            <th>state</th>
            <th className="now__num">for</th>
            <th>what it is doing</th>
            <th>started</th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <Fragment key={run.run_id}>
            <tr data-followable={run.started_here || undefined}>
              <td className="now__entity">
                {run.entity === "" ? <span className="now__unnamed">unnamed</span> : run.entity}
                <span className="now__id">{run.run_id}</span>
              </td>
              {/* La parola porta lo stato quanto la tinta: divieto 5. */}
              <td className="now__state" data-state={run.state}>
                {run.state === "waiting" ? "waits for you" : "working"}
              </td>
              <td className="now__num">{howLong(now - run.since)}</td>
              {/* QUALE PASSO, NON QUANTI. «3 passi aperti» non dice niente:
                  un passo aperto da sei minuti lavora, lo stesso da tre ore e'
                  appeso. Il tentativo si scrive solo se non e' il primo —
                  «2ª volta» su un passo aperto vuol dire che il primo giro e'
                  caduto, ed e' l'informazione che una riga verde nasconde. */}
              <td className="now__steps">
                {run.state === "waiting" ? (
                  "—"
                ) : run.open_now.length === 0 ? (
                  run.open_steps
                ) : (
                  run.open_now.map((step) => (
                    <span className="now__step" key={`${step.step_id}::${String(step.attempt)}`}>
                      {step.step_id}
                      {step.attempt > 1 && <b>attempt {step.attempt}</b>}
                      <i>{howLong(step.open_for_secs)}</i>
                    </span>
                  ))
                )}
              </td>
              {/* SI APRE SOLO CIÒ CHE SI PUÒ DAVVERO APRIRE. Il testo dal vivo
                  di una corsa vive nella memoria del guscio che l'ha avviata:
                  una corsa partita dal terminale si vede qui — ed è tutto il
                  punto di questa schermata — ma non si può ancora seguire. Un
                  pulsante che non apre niente è peggio di nessun pulsante:
                  chi lo preme conclude che la finestra è rotta. Qui la riga
                  dice perché, invece di fingere. */}
              <td className="now__from">
                {run.started_here ? (
                  <button type="button" className="now__open" onClick={() => onOpen(run.run_id)}>
                    watch
                  </button>
                ) : (
                  <span className="now__elsewhere">started elsewhere</span>
                )}
              </td>
            </tr>
            {/* WHAT IT WAITS FOR, AND THE GESTURE THAT ANSWERS IT, under the
                row: a run parked on a person is worth nothing as a line alone. */}
            {run.state === "waiting" && (
              <tr className="now__handed">
                <td colSpan={5}>
                  <Handed runId={run.run_id} />
                </td>
              </tr>
            )}
            </Fragment>
          ))}
        </tbody>
      </table>
    </section>
  );
}
