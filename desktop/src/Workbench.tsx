import { useState } from "react";
import { createContext, useContext } from "react";
import { closeHandedStep } from "./engine";

/**
 * **THE WORK AND THE DECISION IN THE SAME PLACE.** A handed step offered two
 * gestures and no bench: the work itself happened in some terminal the window
 * knew nothing about, in whichever tree it stood. The terminal now opens on
 * the run's own tree, and the mandate and the verdict travel with the pane.
 */

/** A terminal opened to work on one handed step, and what it is for. */
export interface Bench {
  terminalId: string;
  runId: string;
  stepId: string;
  mandate: string;
}

/** How a handed step asks for a bench. `null` outside a window that has one. */
export const BenchContext = createContext<((bench: Bench) => void) | null>(null);

export function useBench(): ((bench: Bench) => void) | null {
  return useContext(BenchContext);
}

interface StripProps {
  bench: Bench;
  /** Called once the engine accepted a verdict: the bench is over. */
  onClosed: (answer: string) => void;
}

/**
 * The strip above a bench's pane: what was asked, and the two ways out. The
 * terminal stays open after it: what was learnt in there is often read next.
 */
export function WorkbenchStrip({ bench, onClosed }: StripProps) {
  const [said, setSaid] = useState("");
  const [busy, setBusy] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);

  const close = (outcome: "went" | "broke") => {
    setBusy(true);
    setTrouble(null);
    closeHandedStep(bench.runId, bench.stepId, outcome, said).then(
      (answer) => {
        setBusy(false);
        onClosed(answer);
      },
      (error: unknown) => {
        setBusy(false);
        setTrouble(String(error));
      },
    );
  };

  return (
    <section className="bench" aria-label={`the step ${bench.stepId} waits on you`}>
      <header className="bench__head">
        <span className="bench__step">{bench.stepId}</span>
        <span className="bench__run">of {bench.runId}</span>
      </header>
      {bench.mandate !== "" && <pre className="bench__mandate">{bench.mandate}</pre>}
      <input
        className="bench__said"
        aria-label="what you did"
        placeholder="what you did, in a line"
        value={said}
        onChange={(event) => setSaid(event.target.value)}
      />
      <div className="bench__acts">
        <button type="button" className="is-primary" disabled={busy} onClick={() => close("went")}>
          it went
        </button>
        <button type="button" disabled={busy} onClick={() => close("broke")}>
          it broke
        </button>
      </div>
      {trouble !== null && <p className="bench__trouble">{trouble}</p>}
    </section>
  );
}
