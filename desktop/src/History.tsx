// The history of runs. **NOT «NOW» WITH MORE ROWS**: «Now» asks the store what
// is open and knows no past, while the question here is what repeats. And a
// column almost nobody draws — the retries: a step repeated and then passed
// counts as gone, and the struggle disappears from a green run.

import { useState } from "react";
import { useAsk, useClock } from "./ask";
import { executionHistory, type Execution, type ModelCall } from "./engine";
import { t } from "./i18n";

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
  broke: "broke",
  open: "open",
  went: "went",
  other: "other",
};

/** Quando è successo, in ore e minuti. La data solo se non è oggi. */
export function whenOf(startedAt: number, now: number): string {
  const then = new Date(startedAt * 1000);
  const today = new Date(now * 1000);
  const sameDay =
    then.getFullYear() === today.getFullYear() &&
    then.getMonth() === today.getMonth() &&
    then.getDate() === today.getDate();
  const time = then.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return time;
  return `${then.toLocaleDateString("en-GB", { day: "2-digit", month: "2-digit" })} ${time}`;
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
  return `$${(micros / 1_000_000).toFixed(3)}`;
}

/** Token visti da una chiamata: quelli che ha dichiarato, non una stima. */
function seenTokens(call: ModelCall): number {
  const parts = [call.input_tokens, call.output_tokens, call.cached_tokens, call.cache_write_tokens];
  const known = parts.filter((part): part is number => part !== null);
  // NESSUN NUMERO NON E' ZERO. Una chiamata che non ha dichiarato token non ne
  // ha consumati zero: non lo sappiamo, e scrivere zero e' la bugia comoda.
  if (known.length === 0) return call.total_tokens ?? -1;
  return known.reduce((sum, part) => sum + part, 0);
}

/**
 * Le chiamate al modello di una corsa, aperte solo se si chiedono.
 *
 * **IL COSTO CALCOLATO E QUELLO DICHIARATO STANNO AFFIANCATI.** Uno lo ricava
 * Sailor dai token, l'altro lo dice il motore: se divergono, il posto in cui
 * accorgersene e' questo. E' il controllo che nella ricognizione del
 * 31/08/2026 manca a Langfuse, LangSmith e Phoenix — tutti e tre con bug
 * pubblici sui numeri, tutti e tre senza una seconda fonte da confrontare.
 */
function Calls({ calls }: { calls: ModelCall[] }) {
  if (calls.length === 0) return null;
  return (
    <details className="calls">
      <summary className="calls__head">
        {calls.length} call{calls.length === 1 ? "" : "s"} to the model
      </summary>
      <table className="now__table">
        <thead>
          <tr>
            <th>step</th>
            <th>engine</th>
            <th>model</th>
            <th className="now__num">tokens</th>
            <th className="now__num">cost</th>
            <th className="now__num">declared</th>
          </tr>
        </thead>
        <tbody>
          {calls.map((call) => {
            const tokens = seenTokens(call);
            return (
              <tr key={call.call_id}>
                <td className="now__when">{call.step_id ?? "—"}</td>
                <td className="now__when">
                  {call.cli === "" ? call.purpose : call.cli}
                  {call.error_type !== null && <span className="now__why">{call.error_type}</span>}
                </td>
                <td className="now__when">
                  {call.actual_model === "" ? t("ui.cost.model_not_declared") : call.actual_model}
                </td>
                <td className="now__num">{tokens < 0 ? "not said" : tokens.toLocaleString("en-GB")}</td>
                <td className="now__num">{call.cost_micros === null ? "not said" : money(call.cost_micros)}</td>
                <td className="now__num">
                  {call.declared_cost_micros === null ? "—" : money(call.declared_cost_micros)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </details>
  );
}

/**
 * The runs of one tree, or all of them. Every tree's runs in one list is a list
 * about nobody: `runs.worktree` is what makes the narrower question askable,
 * and a run recorded before that column has no answer — it is shown under
 * «everywhere» and never counted into a tree it might not belong to.
 */
export function runsIn(runs: Execution[], root: string | null): Execution[] {
  if (root === null) return runs;
  return runs.filter((run) => run.worktree === root);
}

export function History({ native, root }: { native: boolean; root: string | null }) {
  const { asked } = useAsk<Execution[]>(
    native,
    executionHistory,
    REFRESH_MS,
    "outside the shell: the engine reads the history",
  );
  const now = useClock();
  // Narrow by default when there is a tree to narrow to: the question a person
  // asks in a project is about that project.
  const [narrow, setNarrow] = useState(true);

  if (asked.state === "mute") {
    return (
      <div className="now">
        <p className="now__mute">Cannot read the history: {asked.why}</p>
      </div>
    );
  }
  if (asked.state === "asking") {
    return (
      <div className="now">
        <p className="now__mute">Reading the ledger…</p>
      </div>
    );
  }
  if (asked.value.length === 0) {
    return (
      <div className="now">
        <p className="now__empty">The ledger remembers no run.</p>
      </div>
    );
  }

  const here = runsIn(asked.value, narrow ? root : null);
  const shown = here.slice(0, SHOWN);
  return (
    <div className="now">
      <header className="now__head">
        <h2 className="now__title">History</h2>
        <span className="now__count">{here.length}</span>
        <span className="now__note">
          {shown.length < here.length ? `the ${SHOWN} most recent` : "every one the ledger remembers"}
        </span>
        {root !== null && (
          <button
            type="button"
            className="rail__all now__scope"
            data-active={narrow || undefined}
            onClick={() => setNarrow((was) => !was)}
          >
            {narrow ? "this tree" : "everywhere"}
          </button>
        )}
      </header>
      <table className="now__table">
        <thead>
          <tr>
            <th>run</th>
            <th>how it ended</th>
            <th>when</th>
            <th className="now__num">lasted</th>
            <th className="now__num">steps</th>
            <th className="now__num">retried</th>
            <th className="now__num">cost</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((run) => {
            const outcome = outcomeOf(run);
            const row = (
              <tr key={run.run_id}>
                <td className="now__entity">
                  {run.entity === "" ? <span className="now__unnamed">unnamed</span> : run.entity}
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
            // LE CHIAMATE STANNO SOTTO LA CORSA, CHIUSE. Aperte sempre, una
            // corsa con quaranta chiamate seppellirebbe le altre righe; in una
            // pagina a parte, il confronto fra costo calcolato e costo
            // dichiarato costerebbe un viaggio. Chiuse qui e' il compromesso
            // che tiene tutte e due le domande a portata.
            const detail = run.calls.length > 0 && (
              <tr key={`${run.run_id}::calls`} className="now__detail">
                <td colSpan={7}>
                  <Calls calls={run.calls} />
                </td>
              </tr>
            );
            return detail === false ? row : [row, detail];
          })}
        </tbody>
      </table>
    </div>
  );
}
