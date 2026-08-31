// La prima schermata: cosa sta succedendo adesso.
//
// PERCHÉ NON UN ELENCO DI FLUSSI. Fino a stasera la finestra si apriva sulla
// tela, cioè sull'inventario di ciò che *si potrebbe* far girare. È la scuola
// vecchia — n8n, Zapier, Dify aprono così — e ha un difetto preciso: chi
// riapre la finestra dopo un'ora non ha modo di sapere se qualcosa sta ancora
// lavorando, se qualcosa si è rotto, o se qualcosa aspetta lui. Deve andarlo a
// cercare. La generazione nuova di questi strumenti (Temporal, Trigger.dev,
// Inngest, GitHub Actions, Buildkite) apre invece sulle corse, e questa
// schermata sta di là.
//
// QUELLO CHE NESSUNO FA, E CHE QUI SI FA. Nei quindici prodotti confrontati il
// 31/08/2026, «cosa sta girando adesso» si ottiene sempre **filtrando per
// stato una lista storica**: nessuno ha una vista delle sole corse vive. Qui la
// domanda è diretta — `open_runs` chiede al deposito ciò che è aperto — e non
// c'è una lista storica da filtrare, perché non è quella la domanda.
//
// «ASPETTA TE» È UNO STATO, NON UN PALLINO. È la lezione più cara della
// ricognizione: Cursor mostra `1` nel badge mentre la barra laterale ne elenca
// molti di più, e OpenHands usa lo stesso pallino verde lampeggiante per «sta
// lavorando» e per «non ho mai iniziato». Qui l'attesa è un gruppo con un nome,
// in cima, e la parola sta accanto alla tinta perché il divieto 5 della
// direzione visiva non ammette che il colore porti da solo uno stato.

import { useCallback, useEffect, useState } from "react";
import { openRuns, type OpenRun } from "./engine";

/** Ogni quanto si richiede l'elenco al deposito. */
const REFRESH_MS = 4000;

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
  return `${days} g ${hours % 24} h`;
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

/** Come sta l'interrogazione del deposito, dal punto di vista di chi guarda. */
type Asking =
  | { state: "asking" }
  | { state: "answered"; runs: OpenRun[]; at: number }
  | { state: "mute"; why: string };

interface NowProps {
  /** Vero dentro il guscio nativo: fuori non c'è deposito da interrogare. */
  native: boolean;
  /** Aprire la corsa sulla tela, per guardarci dentro. */
  onOpen: (runId: string) => void;
}

export function Now({ native, onOpen }: NowProps) {
  const [asking, setAsking] = useState<Asking>(() =>
    native ? { state: "asking" } : { state: "mute", why: "fuori dal guscio: il deposito lo legge il motore" },
  );
  // L'orologio che fa invecchiare le durate. Senza, «ferma da 2 min» resta
  // scritto per un'ora: la riga sembrerebbe viva e sarebbe congelata.
  const [now, setNow] = useState(() => Date.now() / 1000);

  const ask = useCallback(() => {
    openRuns()
      .then((runs) => setAsking({ state: "answered", runs, at: Date.now() / 1000 }))
      .catch((error: unknown) => setAsking({ state: "mute", why: String(error) }));
  }, []);

  useEffect(() => {
    if (!native) return;
    ask();
    const tick = window.setInterval(ask, REFRESH_MS);
    return () => window.clearInterval(tick);
  }, [native, ask]);

  useEffect(() => {
    const tick = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(tick);
  }, []);

  if (asking.state === "mute") {
    // UN DEPOSITO MUTO SI DICE. Una schermata vuota e un deposito irraggiungibile
    // si assomigliano troppo, e la seconda è quella in cui si continua a lavorare
    // credendo che non stia girando niente.
    return (
      <div className="now">
        <p className="now__mute">Non riesco a chiedere cosa sta girando: {asking.why}</p>
      </div>
    );
  }

  if (asking.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Chiedo al deposito cosa è aperto…</p>
      </div>
    );
  }

  const { waiting, working } = groupRuns(asking.runs);

  if (asking.runs.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">Non sta girando niente, e niente aspetta te.</p>
      </div>
    );
  }

  return (
    <div className="now">
      {waiting.length > 0 && (
        <RunGroup
          title="Aspettano te"
          note="ferme finché non fai qualcosa"
          runs={waiting}
          now={now}
          onOpen={onOpen}
        />
      )}
      {working.length > 0 && (
        <RunGroup title="Al lavoro" note="qualcuno o qualcosa ci sta lavorando" runs={working} now={now} onOpen={onOpen} />
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
            <th>corsa</th>
            <th>stato</th>
            <th className="now__num">da</th>
            <th className="now__num">passi aperti</th>
            <th>avviata</th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <tr key={run.run_id} data-followable={run.started_here || undefined}>
              <td className="now__entity">
                {run.entity === "" ? <span className="now__unnamed">senza nome</span> : run.entity}
                <span className="now__id">{run.run_id}</span>
              </td>
              {/* La parola porta lo stato quanto la tinta: divieto 5. */}
              <td className="now__state" data-state={run.state}>
                {run.state === "waiting" ? "aspetta te" : "in corso"}
              </td>
              <td className="now__num">{howLong(now - run.since)}</td>
              <td className="now__num">{run.state === "waiting" ? "—" : run.open_steps}</td>
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
                    guarda
                  </button>
                ) : (
                  <span className="now__elsewhere">avviata fuori da qui</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
