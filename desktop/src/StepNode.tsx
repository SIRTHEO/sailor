import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { Step, StepKind, StepRun, StepState } from "./flow";

export interface StepNodeData extends Record<string, unknown> {
  step: Step;
  kind: StepKind;
  run?: StepRun;
  /** Il flusso a cui il passo appartiene, e il colore della sua corsia sulla tela unica. */
  flowName: string;
  color: string;
  /** Vero quando un altro flusso è a fuoco: questo passo si ritrae, non sparisce. */
  dimmed: boolean;
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

export const KIND_LABEL: Record<StepKind, string> = {
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
  const { step, kind, run, flowName, color: flowColor, dimmed } = data as StepNodeData;
  const state: StepState = run?.state ?? "waiting";
  const color = STATE_COLOR[state];
  const isAgent = kind === "engine";

  return (
    <div
      className="step-node"
      data-agent={isAgent || undefined}
      data-dimmed={dimmed || undefined}
      style={{
        borderColor: selected ? "#3b82f6" : color,
        borderWidth: selected ? 2 : 1,
      }}
    >
      {/* Il bollino di corsia: a quale flusso appartiene questo passo, nella
          tela dove tutti i flussi stanno insieme. */}
      <div className="step-node__flow" style={{ background: flowColor }} title={flowName} />
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

export interface FlowBandData extends Record<string, unknown> {
  name: string;
  description: string;
  stepCount: number;
  color: string;
  dimmed: boolean;
}

/**
 * La corsia di un flusso: solo lo sfondo e l'etichetta, dietro ai suoi passi.
 * Non ha maniglie e non si seleziona — è la cornice che rende leggibile «un
 * sistema solo, i rami connessi» invece di quaranta nodi sparsi.
 */
export function FlowBandNode({ data }: NodeProps) {
  const { name, description, stepCount, color, dimmed } = data as FlowBandData;
  return (
    <div className="flow-band" data-dimmed={dimmed || undefined} style={{ borderColor: color }}>
      <div className="flow-band__head">
        <span className="flow-band__name" style={{ color }}>
          {name}
        </span>
        <span className="flow-band__count">{stepCount} passi</span>
      </div>
      <div className="flow-band__desc">{description}</div>
    </div>
  );
}
