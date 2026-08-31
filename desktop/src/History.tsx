// La storia delle esecuzioni.
//
// **NON È «ADESSO» CON PIÙ RIGHE.** «Adesso» chiede al deposito ciò che è
// aperto e non conosce il passato. Qui si guarda indietro, e la domanda è
// un'altra: che cosa si ripete. La stessa corsa caduta tre volte di fila si
// vede solo in una vista che tiene le corse chiuse — è la ragione per cui
// Airflow ha una griglia passi × corse e non solo una lista.
//
// UNA COLONNA CHE QUASI NESSUNO METTE: i tentativi. Un passo ripetuto e poi
// riuscito conta come andato, e la corsa risulta verde: la fatica sparisce.
// `steps_retried` la rimette a schermo senza colorare di rosso una corsa che
// rossa non è.

import { useAsk, useClock } from "./ask";
import { executionHistory, type Execution } from "./engine";

/** Ogni quanto si rilegge: la storia cresce piano. */
const REFRESH_MS = 15000;

/** Quante ne mostra. La domanda «cosa si ripete» si esaurisce molto prima. */
const SHOWN = 120;

/**
 * Com'è finita una corsa, in una parola.
 *
 * **ROTTA VINCE SU APERTA**, e non è un dettaglio: una corsa con un passo
 * caduto e un altro ancora in volo è un guasto che sta ancora bruciando.
 * Metterla fra le aperte la toglierebbe dall'occhio di chi cerca i guasti — ed
 * è la stessa regola che il motore applica nel riepilogo di giornata, scritta
 * là in `board.rs`. Se una delle due cambia, cambiano tutte e due.
 */
export function outcomeOf(run: Execution): "broke" | "open" | "went" | "other" {
  if (run.error !== null || run.steps_broke > 0 || ["failed", "broke", "error"].includes(run.status)) {
    return "broke";
  }
  if (run.steps_open.length > 0 || ["running", "open"].includes(run.status)) return "open";
  if (run.status === "succeeded") return "went";
  return "other";
}

const OUTCOME_WORD: Record<ReturnType<typeof outcomeOf>, string> = {
  broke: "rotta",
  open: "aperta",
  went: "andata",
  other: "altro",
};

/** Quando è successo, in ore e minuti. La data solo se non è oggi. */
export function whenOf(startedAt: number, now: number): string {
  const then = new Date(startedAt * 1000);
  const today = new Date(now * 1000);
  const sameDay =
    then.getFullYear() === today.getFullYear() &&
    then.getMonth() === today.getMonth() &&
    then.getDate() === today.getDate();
  const time = then.toLocaleTimeString("it-IT", { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return time;
  return `${then.toLocaleDateString("it-IT", { day: "2-digit", month: "2-digit" })} ${time}`;
}

/** Quanto è durata. `null` è una corsa che non è mai finita, e si dice. */
export function lastedOf(seconds: number | null): string {
  if (seconds === null) return "—";
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

function money(micros: number): string {
  if (micros === 0) return "—";
  return `${(micros / 1_000_000).toLocaleString("it-IT", {
    minimumFractionDigits: 3,
    maximumFractionDigits: 3,
  })} $`;
}

export function History({ native }: { native: boolean }) {
  const { asked } = useAsk<Execution[]>(
    native,
    executionHistory,
    REFRESH_MS,
    "fuori dal guscio: la storia la legge il motore",
  );
  const now = useClock();

  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">Non riesco a leggere la storia: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Leggo il deposito…</p>
      </div>
    );
  }
  if (asked.value.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">Il deposito non ricorda nessuna corsa.</p>
      </div>
    );
  }

  const shown = asked.value.slice(0, SHOWN);
  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">Storia</h2>
        <span className="now__count">{asked.value.length}</span>
        <span className="now__note">
          {shown.length < asked.value.length ? `le ${SHOWN} più recenti` : "tutte quelle che il deposito ricorda"}
        </span>
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>corsa</th>
            <th>com'è finita</th>
            <th>quando</th>
            <th className="now__num">durata</th>
            <th className="now__num">passi</th>
            <th className="now__num">ritentati</th>
            <th className="now__num">costo</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((run) => {
            const outcome = outcomeOf(run);
            return (
              <tr key={run.run_id}>
                <td className="now__entity">
                  {run.entity === "" ? <span className="now__unnamed">senza nome</span> : run.entity}
                  {/* L'ERRORE STA SULLA RIGA, NON DIETRO UN CLIC. Su GitHub
                      Actions «perché è caduta la build» costa un paio di link e
                      migliaia di righe di registro, ed è la lamentela più
                      citata di quel prodotto. Qui la prima riga del motivo si
                      legge da fuori. */}
                  {run.error !== null && <span className="now__why">{run.error}</span>}
                </td>
                <td className="now__state" data-outcome={outcome}>
                  {OUTCOME_WORD[outcome]}
                </td>
                <td className="now__when">{whenOf(run.started_at, now)}</td>
                <td className="now__num">{lastedOf(run.duration_secs)}</td>
                <td className="now__num">
                  {run.steps_went}/{run.steps_total}
                </td>
                {/* Zero non si scrive: una colonna piena di zeri nasconde i
                    numeri che contano. */}
                <td className="now__num">{run.steps_retried === 0 ? "—" : run.steps_retried}</td>
                <td className="now__num">{money(run.total_cost_micros)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
