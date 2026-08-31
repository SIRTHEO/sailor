import { createContext, useContext } from "react";
import { Handle, Position, useStore, type NodeProps } from "@xyflow/react";
import type { Step, StepKind, StepRun, StepState } from "./flow";
import { nodeId } from "./layout";
import { MODEL_KEY, toolOf, useTool, useToolsAreKnown } from "./tools";
import { ToolMark } from "./ToolMark";
import { formatCost, formatTokens, usageIsPartial, type StepUsage } from "./stepusage";

/**
 * Lo stato vero dei passi, chiavato `flusso::passo`.
 *
 * **PASSA DA UN CONTESTO E NON DAI `data` DEL NODO**, per la stessa ragione già
 * scritta accanto all'innesco: i `data` fanno parte dell'elenco dei nodi, e un
 * elenco ricostruito a ogni fatto in arrivo è bastato una volta a mandare la
 * tela in ciclo infinito. Qui i fatti cambiano il valore del contesto, non
 * l'identità dei nodi: si ridisegna quello che è cambiato, e basta.
 */
export const StepRunContext = createContext<Map<string, StepRun>>(new Map());

/**
 * Quanto ha speso ogni passo, e su quale modello, chiavato `flusso::passo`.
 *
 * Passa da un contesto per la stessa ragione dello stato: la spesa si aggiorna
 * ogni tre secondi mentre la corsa gira, e rifare l'elenco dei nodi a ogni
 * aggiornamento è ciò che una volta ha mandato la tela in ciclo infinito.
 */
export const StepUsageContext = createContext<Map<string, StepUsage>>(new Map());

/**
 * Sotto questo zoom un nodo smette di fingersi leggibile.
 *
 * Non è un numero di gusto: a 0,5 — lo zoom che la tela sceglie da sola con due
 * flussi aperti — il nome di una corsia veniva alto 6,5px e la descrizione
 * 5,5px. Restavano disegnati, e nessuno poteva leggerli.
 */
const FAR_ZOOM = 0.62;

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
 *
 * **QUESTI VALORI SONO MISURATI, E I PRECEDENTI NON LO ERANO.** Fino al
 * 31/08/2026 erano `#cbd5e1 #3b82f6 #22c55e #ef4444 #f59e0b #a855f7`, ed erano
 * il colore del testo della parola di stato su ogni nodo: sul fondo delle
 * schede il migliore faceva 3,79:1 e «in attesa» faceva **1,42:1**, cioè la
 * parola più frequente della tela era illeggibile. Adesso il minimo è 5,24:1.
 *
 * Restano in TypeScript perché la minimappa di React Flow vuole una stringa e
 * non legge le variabili CSS. Sulla tela il colore arriva invece dal foglio di
 * stile via `data-state`: la coppia va tenuta allineata a `--state-*` in
 * `styles.css`.
 */
export const STATE_COLOR: Record<StepState, string> = {
  waiting: "#656b63",
  running: "#1a5f96",
  went: "#146447",
  broke: "#a13930",
  capped: "#92531b",
  handed_to_human: "#6b46a8",
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

/** Il modello scelto dal passo, se ne ha scelto uno. */
function modelOf(step: Step): string {
  const model = step.with?.[MODEL_KEY];
  return typeof model === "string" ? model : "";
}

/**
 * Cosa esegue questo passo, sulla tela: il segno dello strumento, come si
 * chiama, e il modello se ne è stato scelto uno.
 *
 * UNO STRUMENTO ASSENTE NON SPARISCE, si spegne. Un nodo che su questa macchina
 * non può girare deve dirlo guardandolo — col motivo che il rilevamento ha
 * già in mano — invece di sembrare a posto e fallire alla partenza. E finché
 * la scoperta non ha risposto non si accusa nessuno: si mostra
 * l'identificativo scritto nel passo, che è quello che il file dice.
 */
function StepTool({ id, model, actual }: { id: string; model: string; actual: string[] }) {
  const tool = useTool(id);
  const known = useToolsAreKnown();
  // Tre stati, non due: trovato e c'è, trovato e non c'è, e «non lo so
  // ancora». Il terzo non è il secondo, e colorarlo di spento sarebbe una
  // bugia che dura quanto il rilevamento.
  const off = known && !tool?.available;
  const why = tool?.reason ?? (known ? "non è fra gli strumenti rilevati su questa macchina" : "");
  // Il modello vero si mostra solo quando dice qualcosa di diverso da quello
  // scritto nel passo: ripetere due volte la stessa parola non informa.
  const surprising = actual.filter((name) => name !== model);

  return (
    <div className="step-node__tool" data-off={off || undefined}>
      <ToolMark id={id} size={16} off={off} />
      <div className="step-node__tool-text">
        <span className="step-node__tool-name" title={id}>
          {tool?.name ?? id}
        </span>
        {model !== "" && <span className="step-node__tool-model">{model}</span>}
        {surprising.length > 0 && (
          <span
            className="step-node__tool-actual"
            title={`il motore ha risposto di aver usato: ${surprising.join(", ")}`}
          >
            ha girato su {surprising.join(", ")}
          </span>
        )}
        {/* Il motivo si legge sulla tela, non solo passandoci sopra: chi guarda
            un nodo spento non deve andare a cercare dove sta scritto perché. */}
        {off && why !== "" && (
          <span className="step-node__tool-why" title={why}>
            {why}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * Il riquadro che dice **nessun motore**, quando il passo non ne dichiara uno.
 *
 * Prima, senza strumento, il riquadro semplicemente non compariva: un passo che
 * gira qui sulla macchina era indistinguibile da un passo di cui non si era
 * ancora guardato il motore. L'assenza è un fatto e va disegnata — è il vincolo
 * «chiarezza per chi guarda» nella sua forma più letterale.
 */
function NoTool({ action }: { action: string }) {
  return (
    <div className="step-node__tool" data-none>
      <div className="step-node__tool-text">
        <span className="step-node__tool-name">nessun motore</span>
        <span className="step-node__tool-model" title={action}>
          {action}
        </span>
      </div>
    </div>
  );
}

/**
 * Cosa è entrato, cosa è uscito e quanto è costato — per questo passo.
 *
 * Compare solo dopo che il passo ha chiamato qualcuno: prima non c'è niente da
 * dire, e una riga di zeri sembrerebbe una misura.
 */
function StepMeter({ usage }: { usage: StepUsage }) {
  const partial = usageIsPartial(usage);
  return (
    <div className="step-node__meter">
      <span title={`${usage.inputTokens} token entrati`}>↑ {formatTokens(usage.inputTokens)}</span>
      <span title={`${usage.outputTokens} token usciti`}>↓ {formatTokens(usage.outputTokens)}</span>
      {usage.costMicros !== null && (
        <span
          className={partial ? "step-node__cost step-node__partial" : "step-node__cost"}
          title={
            partial
              ? `${usage.callsWithoutCost} chiamate su ${usage.calls} non dichiarano un costo: il conto è più basso del vero`
              : `${usage.calls} chiamate`
          }
        >
          {formatCost(usage.costMicros)}
          {partial && " +"}
        </span>
      )}
      {/* Nessuna chiamata ha dichiarato un costo: si dice, invece di mostrare
          uno zero che passerebbe per una misura. */}
      {usage.costMicros === null && (
        <span
          className="step-node__cost step-node__partial"
          title="nessuna delle chiamate di questo passo dichiara un costo"
        >
          costo non dichiarato
        </span>
      )}
    </div>
  );
}

export function StepNode({ data, selected }: NodeProps) {
  const { step, kind, run: fromData, flowName, color: flowColor, dimmed } = data as StepNodeData;
  // I FATTI VERI VINCONO SULL'ESEMPIO. `run` nei `data` esiste ancora perché è
  // così che i dati d'esempio colorano la tela fuori dal guscio nativo; quando
  // una corsa vera esiste, è la sua a contare.
  const key = nodeId(flowName, step.id);
  const run = useContext(StepRunContext).get(key) ?? fromData;
  const usage = useContext(StepUsageContext).get(key);
  const state: StepState = run?.state ?? "waiting";
  const isAgent = kind === "engine";
  // Quale strumento esegue questo nodo si legge sulla tela, non solo aprendo il
  // pannello: è la prima domanda di chi guarda un flusso fatto da altri.
  const tool = toolOf(step.with);
  // Quanto è lontano l'occhio. Sotto la soglia il nodo lascia cadere il
  // dettaglio invece di disegnarlo alto sei pixel.
  const zoom = useStore((s) => s.transform[2]);
  const far = zoom < FAR_ZOOM;

  return (
    <div
      className="step-node"
      data-agent={isAgent || undefined}
      data-dimmed={dimmed || undefined}
      data-state={state}
      data-selected={selected || undefined}
      data-far={far || undefined}
    >
      {/* Il bollino di corsia: a quale flusso appartiene questo passo, nella
          tela dove tutti i flussi stanno insieme. */}
      <div className="step-node__flow" style={{ background: flowColor }} title={flowName} />
      <Handle type="target" position={Position.Left} />

      {!far && (
        <div className="step-node__head">
          <span className="step-node__kind">{KIND_LABEL[kind]}</span>
          {step.when && <span className="step-node__when">condizionato</span>}
        </div>
      )}

      <div className="step-node__id">{step.id}</div>

      {/* DA VICINO IL NODO DICE SEMPRE CON COSA GIRA — anche quando la risposta
          è «con niente». Da lontano no: a quello zoom non si leggerebbe. */}
      {!far && (tool !== "" ? (
        <StepTool id={tool} model={modelOf(step)} actual={usage?.models ?? []} />
      ) : (
        <NoTool action={step.action} />
      ))}

      {!far && usage && usage.calls > 0 && <StepMeter usage={usage} />}

      <div className="step-node__foot">
        <span className="step-node__state">{STATE_LABEL[state]}</span>
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
  // Come per un passo: da lontano l'etichetta cresce invece di restare
  // disegnata alta sei pixel.
  const far = useStore((s) => s.transform[2]) < FAR_ZOOM;
  // LA TINTA DELLA CORSIA ESCE QUANDO LA CORSIA NON È A FUOCO, e il bordo torna
  // al filo neutro. Prima a ritrarsi era `opacity: 0.3` su tutta la corsia, cioè
  // anche sulle sue parole: la descrizione faceva 1,47:1. Quale ramo si sta
  // guardando si vede dai segni — il velo, il bordo, il bollino — mai dal testo.
  const borderColor = dimmed ? undefined : color;
  return (
    <div
      className="flow-band"
      data-dimmed={dimmed || undefined}
      data-far={far || undefined}
      style={{ borderColor }}
    >
      <div className="flow-band__head">
        {/* IL COLORE DELLA CORSIA È UN SEGNO, NON UN COLORE DI TESTO.
            Scriverci sopra il nome del flusso è ciò che si faceva prima, e la
            tavolozza delle corsie non era mai stata misurata: su quattordici
            tinte solo tre reggono 4,5:1 sul fondo della banda, e cinque stanno
            sotto 3:1. `prima-corsa` veniva 2,77:1. Il nome adesso è inchiostro
            e si legge sempre; la tinta identifica la corsia da qui. */}
        <span className="flow-band__mark" style={{ background: color }} aria-hidden="true" />
        <span className="flow-band__name">{name}</span>
        <span className="flow-band__count">{stepCount} passi</span>
      </div>
      {/* Troncata a due righe dallo stile, ma non persa: per intero si legge
          passandoci sopra. */}
      <div className="flow-band__desc" title={description}>
        {description}
      </div>
    </div>
  );
}
