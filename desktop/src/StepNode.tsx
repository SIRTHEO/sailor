import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { Step, StepKind, StepRun, StepState } from "./flow";

export interface StepNodeData extends Record<string, unknown> {
  step: Step;
  kind: StepKind;
  run?: StepRun;
}

/**
 * Il colore dice come è finito il passo, e i finali non sono intercambiabili:
 * «fermo al tetto» non è «rotto» — nessuno lo ritenterà — e «aspetta una
 * persona» non è un guasto. Dare loro lo stesso colore è dire una bugia.
 */
const STATE_COLOR: Record<StepState, string> = {
  waiting: "#cbd5e1",
  running: "#3b82f6",
  went: "#22c55e",
  broke: "#ef4444",
  capped: "#f59e0b",
  handed_to_human: "#a855f7",
};

const STATE_LABEL: Record<StepState, string> = {
  waiting: "in attesa",
  running: "in corso",
  went: "andato",
  broke: "rotto, si ritenta",
  capped: "fermo al tetto",
  handed_to_human: "aspetta una persona",
};

const KIND_LABEL: Record<StepKind, string> = {
  trigger: "innesco",
  engine: "agente",
  check: "verifica",
  wait: "attesa",
  branch: "ramo",
  deposit: "deposito",
  gesture: "gesto",
  human: "a una persona",
  subflow: "sotto-flusso",
};

export function StepNode({ data, selected }: NodeProps) {
  const { step, kind, run } = data as StepNodeData;
  const state: StepState = run?.state ?? "waiting";
  const color = STATE_COLOR[state];
  const isAgent = kind === "engine";

  return (
    <div
      className="step-node"
      data-agent={isAgent || undefined}
      style={{
        borderColor: selected ? "#3b82f6" : color,
        borderWidth: selected ? 2 : 1,
      }}
    >
      <Handle type="target" position={Position.Left} />

      <div className="step-node__head">
        <span className="step-node__kind">{KIND_LABEL[kind]}</span>
        {step.when && <span className="step-node__when">condizionato</span>}
      </div>

      <div className="step-node__id">{step.id}</div>

      <div className="step-node__foot">
        <span style={{ color }}>{STATE_LABEL[state]}</span>
        {run && run.attempt > 1 && (
          <span className="step-node__attempt">
            {run.attempt}ª di {step.max_attempts}
          </span>
        )}
      </div>

      {/* Un nodo che possiede un agente non rimanda altrove: i gesti stanno
          addosso al nodo, perché è lì che chi guarda li cerca. */}
      {isAgent && state === "running" && (
        <div className="step-node__agent">
          <span className="step-node__pid">pid {run?.held_by_pid ?? "?"}</span>
          <div className="step-node__actions">
            <button type="button">apri</button>
            <button type="button">sospendi</button>
            <button type="button" className="is-stop">
              termina
            </button>
          </div>
        </div>
      )}

      <Handle type="source" position={Position.Right} />
    </div>
  );
}
