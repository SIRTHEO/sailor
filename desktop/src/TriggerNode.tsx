import { createContext, useContext } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { FlowTrigger, RunSnapshot } from "./engine";

/**
 * Il nodo di innesco: il punto da cui un flusso parte, e il gesto che lo fa
 * partire.
 *
 * ## PERCHÉ È UN NODO E NON SOLO UN PULSANTE NELLA BARRA
 *
 * Un pulsante in cima alla finestra fa partire «il flusso a fuoco», e chi
 * guarda deve ricordarsi quale sia. Su una tela dove tutti i flussi stanno
 * insieme è un modo di lanciare la cosa sbagliata. Il gesto sta invece dove
 * comincia il grafo, addosso al ramo che riguarda: si vede a quale flusso
 * appartiene senza chiederlo, e la consegna che si scrive è la sua.
 *
 * ## QUESTO NODO È IL GESTO, NON IL PASSO
 *
 * Il contratto dell'innesco è nato in `crates/flow` il 28/08/2026, mentre
 * questo pannello si scriveva: un passo senza dipendenze con
 * `"action": "trigger"` e `"with": {"source": "manual"}`, che riceve la
 * consegna in `inputs.<passo>.text`. Quel passo è nel file del flusso e si
 * disegna come tutti gli altri.
 *
 * Il nodo qui sotto **non è quel passo**: è il gesto che gli manda il segnale.
 * Un flusso può anche non avere un passo di innesco — in `crates/flow` un
 * flusso senza `schedule` «gira quando qualcuno lo chiede», che è un fatto e
 * non un vuoto da riempire — e allora questo nodo resta l'unico punto di
 * partenza visibile. Non si salva, non si collega a mano e non compare nel
 * `.flow.json`: è la finestra che lo disegna, e il confine fra ciò che il
 * motore conosce e ciò che la finestra aggiunge deve restare visibile.
 *
 * ## LO STATO ARRIVA DA UN CONTESTO, NON DAI `data` DEL NODO
 *
 * Mettere la corsa nei `data` significherebbe ricostruire l'elenco dei nodi a
 * ogni fatto che arriva dal guscio — e il 28/08/2026 una tela che ricostruiva i
 * nodi dentro un effetto è entrata in un ciclo di render che non si fermava mai
 * abbastanza per misurarsi: nodi invisibili e minimappa piena. I `data` restano
 * due stringhe stabili; quello che cambia passa dal contesto, che ridisegna
 * questo nodo e basta.
 */

/** Come sta l'interrogazione al guscio su come si innesca un flusso. */
export type TriggerState =
  | { state: "asking" }
  | { state: "ready"; trigger: FlowTrigger }
  | { state: "mute"; why: string };

export interface RunControls {
  /** Vero dentro il guscio: fuori non c'è motore, e il pulsante lo dice. */
  native: boolean;
  triggerOf: (flowName: string) => TriggerState;
  /** La corsa più recente di quel flusso, se questa finestra ne conosce una. */
  runOf: (flowName: string) => RunSnapshot | undefined;
  /**
   * La consegna in scrittura. Vive fuori dal nodo perché il nodo si rimonta
   * ogni volta che la tela si ridisegna, e un testo lungo scritto a mano non
   * deve sparire perché qualcuno ha rinominato un passo altrove.
   */
  mandateOf: (flowName: string) => string;
  onMandate: (flowName: string, text: string) => void;
  onRun: (flowName: string) => void;
  starting: (flowName: string) => boolean;
  errorOf: (flowName: string) => string | undefined;
  /** Apre la vista d'esecuzione su quel flusso. */
  onWatch: (flowName: string) => void;
}

const MUTE: RunControls = {
  native: false,
  triggerOf: () => ({ state: "mute", why: "fuori dal guscio: nessun motore da innescare" }),
  runOf: () => undefined,
  mandateOf: () => "",
  onMandate: () => {},
  onRun: () => {},
  starting: () => false,
  errorOf: () => undefined,
  onWatch: () => {},
};

export const RunContext = createContext<RunControls>(MUTE);

export interface TriggerNodeData extends Record<string, unknown> {
  flowName: string;
  color: string;
}

/** L'identificativo del nodo di innesco di un flusso, distinto dai passi. */
export function triggerNodeId(flowName: string): string {
  return `trigger::${flowName}`;
}

const RUNNING_LABEL: Record<string, string> = {
  running: "in corso",
  complete: "completato",
  failed: "fallito",
  waiting: "in attesa",
  stopped: "fermato",
  // Non è «fallito»: la corsa ha rispettato un limite che qualcuno le ha
  // messo. Chi le vede uguali smette di guardare tutte e due.
  cap_reached: "fermato dal tetto di spesa",
  incomplete: "incompleto",
};

export function TriggerNode({ data }: NodeProps) {
  const { flowName, color } = data as TriggerNodeData;
  const controls = useContext(RunContext);
  const trigger = controls.triggerOf(flowName);
  const run = controls.runOf(flowName);
  const busy = controls.starting(flowName) || run?.status === "running";
  const error = controls.errorOf(flowName);

  const mandate = trigger.state === "ready" ? trigger.trigger.mandate : null;
  const canWriteMandate = mandate?.kind === "field";

  return (
    <div className="trigger-node" style={{ borderColor: color }}>
      <div className="trigger-node__head">
        <span className="trigger-node__mark" style={{ background: color }} />
        <span className="trigger-node__kind">innesco · manuale</span>
      </div>

      <div className="trigger-node__flow">{flowName}</div>

      {/* La pianificazione non è un dettaglio da nascondere: se il flusso parte
          anche da solo, chi preme deve sapere che non è l'unico a farlo. */}
      {trigger.state === "ready" && trigger.trigger.scheduled && (
        <div className="trigger-node__note">questo flusso ha anche una pianificazione propria</div>
      )}

      {trigger.state === "asking" && <div className="trigger-node__note">chiedo al motore…</div>}

      {trigger.state === "mute" && <div className="trigger-node__why">{trigger.why}</div>}

      {trigger.state === "ready" && canWriteMandate && (
        // `nodrag` e `nowheel`: senza, trascinare per selezionare il testo
        // sposterebbe il nodo, e la rotellina zoomerebbe la tela invece di
        // scorrere il testo.
        <textarea
          className="trigger-node__mandate nodrag nowheel"
          placeholder={
            mandate?.kind === "field"
              ? `la consegna: entra in «${mandate.step}» come «${mandate.field}»`
              : "la consegna: cosa deve fare, questa volta"
          }
          aria-label={`consegna per il flusso ${flowName}`}
          value={controls.mandateOf(flowName)}
          disabled={busy}
          onChange={(event) => controls.onMandate(flowName, event.target.value)}
        />
      )}

      {trigger.state === "ready" && mandate?.kind === "none" && (
        // Perché non si può scrivere una consegna si dice **prima** di premere.
        // Dopo sarebbe la scoperta che il flusso è partito su un testo altrui.
        <div className="trigger-node__why">{mandate.why}</div>
      )}

      <div className="trigger-node__foot">
        <button
          type="button"
          className="trigger-node__go nodrag"
          disabled={!controls.native || trigger.state !== "ready" || busy}
          onClick={() => controls.onRun(flowName)}
          title={
            controls.native
              ? `fai partire «${flowName}»`
              : "fuori dal guscio nativo non c'è un motore che esegua"
          }
        >
          {busy ? "in corso…" : "▶ Esegui"}
        </button>

        {run && (
          <button
            type="button"
            className="trigger-node__watch nodrag"
            onClick={() => controls.onWatch(flowName)}
            data-status={run.status}
          >
            {RUNNING_LABEL[run.status] ?? run.status}
          </button>
        )}
      </div>

      {error && <div className="trigger-node__error">{error}</div>}

      <Handle type="source" position={Position.Right} />
    </div>
  );
}
