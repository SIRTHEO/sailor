import { useEffect, useState } from "react";
import { stepHistory, type StepPassage } from "./engine";
import { OUTCOME_LABEL, whyFailed } from "./RunConsole";
import { StepRefusal } from "./StepRefusal";

/**
 * What came into this node, over time. The run view tells today's call; this
 * tells all the others — when, where the run started, with what mandate, and
 * what entered **this** step that time.
 *
 * **THE SYSTEM WRITES IT, NOT AN AGENT.** The engine records a step's input the
 * instant it opens it, and `crates/ledger` is append-only. This panel reads.
 *
 * **THE BOUNDARY: WHERE A CALL CAME FROM, YES — WHAT RAN IN A TERMINAL, NO.**
 * Keys, tokens and private paths pass through a session, and nobody expects
 * that from a panel showing a list of dates. This window records nothing new:
 * it shows what the store already keeps in order to execute. Keeping terminal
 * content would have to be an explicit choice, visible, and off by default.
 */

function when(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

function lasted(from: number, to: number | null): string {
  if (to === null) return "still going";
  const delta = Math.max(0, to - from);
  if (delta < 60) return `${delta}s`;
  return `${Math.floor(delta / 60)}m ${delta % 60}s`;
}

/** Un testo lungo si mostra a spicchi: aprire il pannello non è chiedere tutto. */
function shorten(text: string, max = 220): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length <= max ? flat : `${flat.slice(0, max)}…`;
}

interface StepHistoryProps {
  flowName: string;
  stepId: string;
}

type Ask =
  | { state: "asking" }
  | { state: "ready"; passages: StepPassage[] }
  | { state: "mute"; why: string };

export function StepHistory({ flowName, stepId }: StepHistoryProps) {
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    let dropped = false;
    setAsk({ state: "asking" });
    stepHistory(flowName, stepId)
      .then((passages) => {
        if (!dropped) setAsk({ state: "ready", passages });
      })
      .catch((error: unknown) => {
        if (!dropped) setAsk({ state: "mute", why: String(error) });
      });
    return () => {
      dropped = true;
    };
  }, [flowName, stepId]);

  return (
    <section className="history">
      <div className="history__title">What came into this step</div>

      {ask.state === "asking" && <div className="history__note">asking the ledger…</div>}

      {ask.state === "mute" && <div className="history__note">{ask.why}</div>}

      {ask.state === "ready" && ask.passages.length === 0 && (
        // Un elenco vuoto ha due cause diverse, e confonderle manda a cercare
        // un guasto: qui è sempre la prima, perché un errore di lettura arriva
        // come `mute`.
        <div className="history__note">this step has never been passed through</div>
      )}

      {ask.state === "ready" &&
        ask.passages.map((passage) => {
          const key = `${passage.run_id}:${passage.attempt}`;
          const isOpen = open === key;
          return (
            <article className="passage" key={key} data-outcome={passage.outcome ?? "open"}>
              <button
                type="button"
                className="passage__head"
                onClick={() => setOpen(isOpen ? null : key)}
                aria-expanded={isOpen}
              >
                <span className="passage__when">{when(passage.started_at)}</span>
                <span className="passage__outcome">
                  {passage.outcome ? (OUTCOME_LABEL[passage.outcome] ?? passage.outcome) : "still going"} ·{" "}
                  {lasted(passage.started_at, passage.ended_at)}
                </span>
              </button>

              {/* Da dove è partita, e chi l'ha mandata. Sono due fatti
                  diversi: il primo è come il programma è stato invocato — dalla
                  finestra, dalla riga di comando, da una pianificazione — il
                  secondo è quello che il segnale stesso portava scritto. Un
                  campo che il segnale non sapeva non si mostra vuoto: si
                  omette, perché un'etichetta senza valore accanto fa credere
                  che il dato ci sia. */}
              <div className="passage__origin">
                {passage.started_by}
                {passage.signal_where && ` · from ${passage.signal_where}`}
                {passage.signal_who && ` · ${passage.signal_who}`}
              </div>

              {passage.attempt > 1 && (
                <div className="passage__attempt">attempt {passage.attempt} of this run</div>
              )}

              {passage.mandate && (
                <div className="passage__mandate" title={passage.mandate}>
                  <span className="passage__label">mandate</span>
                  {shorten(passage.mandate)}
                </div>
              )}

              {isOpen && (
                <div className="passage__detail">
                  <div className="passage__label">came in</div>
                  <pre className="passage__code">{JSON.stringify(passage.input, null, 2)}</pre>

                  {passage.output !== null && passage.output !== undefined && (
                    <>
                      <div className="passage__label">came out</div>
                      <pre className="passage__code">{JSON.stringify(passage.output, null, 2)}</pre>
                    </>
                  )}

                  {passage.said && (
                    <>
                      <div className="passage__label">said</div>
                      <pre className="passage__code">{passage.said}</pre>
                    </>
                  )}

                  {passage.refusal && <StepRefusal refusal={passage.refusal} />}
                  {passage.failure_class && (
                    <div className="passage__failure">{whyFailed(passage.failure_class)}</div>
                  )}

                  <div className="passage__run">run {passage.run_id}</div>
                </div>
              )}
            </article>
          );
        })}
    </section>
  );
}
