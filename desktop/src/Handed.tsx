import { useCallback, useEffect, useState } from "react";
import { closeHandedStep, handedSteps, takeHandedStep, type HandedStep } from "./engine";
import { BORN_COLS, BORN_ROWS, openTerminal } from "./terminal";
import { useBench } from "./Workbench";

/**
 * The steps of a run that wait for a person, and the two gestures on them:
 * take it, and close it with the outcome you declare. Both are the engine's
 * own commands, shown as they answered — a refusal included.
 */

type Ask =
  | { state: "asking" }
  | { state: "ready"; steps: HandedStep[] }
  | { state: "mute"; why: string };

interface HandedProps {
  runId: string;
  /** Called after a gesture the engine accepted, so whoever lists runs reads again. */
  onChanged?: () => void;
}

export function Handed({ runId, onChanged }: HandedProps) {
  const bench = useBench();
  const [ask, setAsk] = useState<Ask>({ state: "asking" });
  const [said, setSaid] = useState<Record<string, string>>({});
  const [report, setReport] = useState<string | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);

  const read = useCallback(() => {
    handedSteps(runId).then(
      (steps) => setAsk({ state: "ready", steps }),
      (error) => setAsk({ state: "mute", why: String(error) }),
    );
  }, [runId]);

  useEffect(() => {
    read();
  }, [read]);

  const act = (work: Promise<string>) => {
    setTrouble(null);
    work.then(
      (answer) => {
        setReport(answer);
        read();
        onChanged?.();
      },
      (error) => setTrouble(String(error)),
    );
  };

  if (ask.state === "asking") return <p className="handed__note">Asking which steps wait…</p>;
  if (ask.state === "mute") return <p className="handed__note">Cannot read the handed steps: {ask.why}</p>;

  return (
    <div className="handed">
      {ask.steps.length === 0 && (
        <p className="handed__note">No step of this run is handed to a person: it waits on something else.</p>
      )}
      {ask.steps.map((step) => (
        <article className="handed__step" key={step.step_id}>
          <header className="handed__head">
            <span className="handed__id">{step.step_id}</span>
            {step.holder && <span className="handed__holder">offered to «{step.holder}»</span>}
          </header>
          {step.mandate ? (
            <pre className="handed__mandate">{step.mandate}</pre>
          ) : (
            <p className="handed__note">The step declares no mandate.</p>
          )}
          <div className="handed__acts">
            <button type="button" onClick={() => act(takeHandedStep(runId, step.step_id))}>
              take it
            </button>
            {/* **THE TREE THE RUN WAS BORN IN, NOT THE ONE THE WINDOW STANDS
                IN.** A bench in the wrong tree is found out at the first
                command that reads a file, and by then a verdict was given. */}
            {bench !== null && step.worktree !== null && (
              <button
                type="button"
                onClick={() => {
                  setTrouble(null);
                  openTerminal({
                    workspaceRoot: step.worktree ?? "",
                    cols: BORN_COLS,
                    rows: BORN_ROWS,
                  }).then(
                    (born) =>
                      bench({
                        terminalId: born.id,
                        runId,
                        stepId: step.step_id,
                        mandate: step.mandate,
                      }),
                    (error: unknown) => setTrouble(String(error)),
                  );
                }}
              >
                open it as a terminal
              </button>
            )}
          </div>
          <textarea
            className="handed__said"
            aria-label={`what you did for ${step.step_id}`}
            placeholder="what you did, in a line or two"
            value={said[step.step_id] ?? ""}
            onChange={(event) => setSaid({ ...said, [step.step_id]: event.target.value })}
          />
          <div className="handed__acts">
            <button
              type="button"
              className="is-primary"
              onClick={() => act(closeHandedStep(runId, step.step_id, "went", said[step.step_id] ?? ""))}
            >
              close: it went
            </button>
            <button
              type="button"
              onClick={() => act(closeHandedStep(runId, step.step_id, "broke", said[step.step_id] ?? ""))}
            >
              close: it broke
            </button>
          </div>
        </article>
      ))}
      {report && <pre className="handed__report">{report}</pre>}
      {trouble && <p className="handed__trouble">{trouble}</p>}
    </div>
  );
}
