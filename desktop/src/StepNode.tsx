import { createContext, useContext } from "react";
import { Handle, Position, useStore, type NodeProps } from "@xyflow/react";
import type { Step, StepKind, StepRun, StepState } from "./flow";
import { nodeId, type PortShape, type StepPort, type StepPorts } from "./layout";
import { MODEL_KEY, TOOL_KEY, useTool, useToolsAreKnown } from "./tools";
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
  /** Cosa entra e cosa esce, letto dal grafo. Assente solo nei dati di prova. */
  ports?: StepPorts;
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

/**
 * **I DUE REGISTRI DELL'ATTENZIONE, E PRIMA CE N'ERA UNO SOLO.**
 *
 * Fino a oggi ogni passo `running` di un agente si portava addosso un riquadro
 * con tre pulsanti, uno dei quali rosso pieno. Con più agenti in parallelo —
 * che è il caso normale — ogni corsa viva chiedeva attenzione, cioè nessuna la
 * otteneva. Von Restorff funziona solo se **pochissimi** elementi deviano, e
 * il nome dell'errore è *isolation inflation*.
 *
 * Adesso i registri sono due. Le corse vive condividono **lo stesso**
 * indicatore quieto — il punto che respira, in `styles.css` — che dice «vivo»
 * e non «guardami». L'isolamento è riservato a **una cosa sola per vista**:
 * ciò che non si sbloccherà da solo. Se ne aspettano più d'una, si ordinano
 * per gravità e le altre diventano un contatore sul nodo isolato.
 *
 * I due stati qui sotto sono quelli che una persona deve toccare: «aspetta una
 * persona» lo dice da sé, e «fermo al tetto» è il passo che nessuno ritenterà.
 * «rotto, si ritenta» NON è fra questi: si ritenta da solo, e isolarlo sarebbe
 * chiedere un gesto che non serve.
 */
const GESTURE_GRAVITY: Partial<Record<StepState, number>> = {
  handed_to_human: 0,
  capped: 1,
};

export interface GestureCall {
  /** La chiave `flusso::passo` del solo nodo isolato, o `null` se nessuno chiama. */
  key: string | null;
  /** Quante cose aspettano una persona in tutto, compresa quella isolata. */
  waiting: number;
}

/**
 * Chi, fra tutte le corse che la finestra conosce, si prende l'isolamento.
 *
 * Sta qui e non nella disposizione perché legge lo **stato vero** delle corse,
 * che passa da un contesto: calcolarlo dove si costruisce l'elenco dei nodi lo
 * renderebbe vecchio di un fatto.
 */
export function stepThatCallsForAGesture(runs: Map<string, StepRun>): GestureCall {
  const calling = Array.from(runs.entries()).filter(
    ([, run]) => GESTURE_GRAVITY[run.state] !== undefined,
  );
  if (calling.length === 0) return { key: null, waiting: 0 };
  // A parità di gravità decide il nome, che non cambia da un fatto all'altro:
  // un isolamento che salta di nodo a ogni aggiornamento non è un isolamento.
  calling.sort(([leftKey, left], [rightKey, right]) => {
    const gravity =
      (GESTURE_GRAVITY[left.state] as number) - (GESTURE_GRAVITY[right.state] as number);
    return gravity !== 0 ? gravity : leftKey.localeCompare(rightKey);
  });
  return { key: calling[0][0], waiting: calling.length };
}

/** Come si chiama, a parole, il tipo che una forma porta. */
const SHAPE_LABEL: Record<PortShape, string> = {
  text: "testo",
  structure: "struttura",
  value: "valore",
};

/**
 * Una porta: la forma dice il tipo, il pieno dice se è cablata.
 *
 * **IL COLORE NON PORTA NIENTE DA SOLO** (divieto 5). Cerchio, rombo e
 * quadrato si distinguono in bianco e nero; vuoto e pieno pure; e un ingresso
 * obbligatorio che nessuno alimenta aggiunge **la parola**, perché una forma
 * vuota dice «non collegata» ma non dice «e doveva esserlo».
 */
function Port({ port }: { port: StepPort }) {
  const missing = port.required && !port.wired;
  return (
    <span
      className="step-node__port"
      data-wired={port.wired || undefined}
      data-missing={missing || undefined}
      title={`${port.name} · ${SHAPE_LABEL[port.shape]} · ${port.wired ? "collegata" : "non collegata"}`}
    >
      <span className="step-node__port-mark" data-shape={port.shape} aria-hidden="true" />
      <span className="step-node__port-name">{missing ? `${port.name} manca` : port.name}</span>
    </span>
  );
}

/**
 * Le porte del nodo: gli ingressi a sinistra, l'uscita a destra, dalla parte
 * da cui i fili entrano ed escono davvero.
 */
function StepPortsRow({ ports }: { ports: StepPorts }) {
  return (
    <div className="step-node__ports">
      <div className="step-node__ports-column">
        {ports.inputs.map((port) => (
          <Port key={port.name} port={port} />
        ))}
      </div>
      <div className="step-node__ports-column" data-out>
        <Port port={ports.output} />
      </div>
    </div>
  );
}

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
 * I motori che il passo nomina, **nell'ordine in cui li nomina**.
 *
 * **`tool` NON È SOLO UNA STRINGA, E LEGGERLO COSÌ FACEVA DIRE UNA BUGIA AL
 * NODO.** In `crates/actions/src/lib.rs` il campo è un `ToolChoice`, cioè
 * `One(String)` **oppure** `Chain(Vec<String>)`: una catena è «chi eseguire, in
 * ordine di preferenza, il migliore per primo». `toolOf` restituisce il testo o
 * niente, quindi su una catena rispondeva niente, e il nodo disegnava il
 * riquadro «*nessun motore*» sopra la parola `external_engine`. Sui dieci
 * flussi di `flows/` sono **20 passi su 25**: 25 passi `external_engine` in
 * tutto, 20 con una catena e 5 con una stringa sola (`codex`, `agy`, `cargo`,
 * `git`, `git`). La tela diceva «nessun motore» a chi ne aveva dichiarati tre.
 *
 * Il primo della catena è quello che si mostra, perché è quello che il motore
 * proverà per primo; gli altri sono ricambi e si dicono come tali.
 */
export function enginesOf(params: Record<string, unknown> | null | undefined): string[] {
  const declared = params?.[TOOL_KEY];
  if (typeof declared === "string") return declared === "" ? [] : [declared];
  if (!Array.isArray(declared)) return [];
  return declared.filter((id): id is string => typeof id === "string" && id !== "");
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
function StepTool({
  id,
  model,
  actual,
  fallbacks,
}: {
  id: string;
  model: string;
  actual: string[];
  /** I ricambi dichiarati dopo il primo, nell'ordine scritto nel passo. */
  fallbacks: string[];
}) {
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
        {/* NOME IN PROSA, IDENTIFICATIVO IN MONOSPAZIO — e questo slot mostra
            l'uno o l'altro. Finché la scoperta non ha risposto qui ci finisce
            `id`, che è un dato: senza distinguerli, `claude-code` compariva in
            due caratteri diversi sulle due righe dello stesso riquadro — in
            prosa sopra, in monospazio nella catena sotto. La regola non cambia,
            cambia il fatto che si legge prima di applicarla. */}
        <span className="step-node__tool-name" data-raw={tool === undefined || undefined} title={id}>
          {tool?.name ?? id}
        </span>
        {model !== "" && <span className="step-node__tool-model">{model}</span>}
        {/* UNA CATENA SI DICE, e prima non si diceva affatto: il nodo mostrava
            «nessun motore» proprio dove il passo ne nominava tre. Chi guarda
            deve sapere che se il primo non c'è ne parte un altro. */}
        {fallbacks.length > 0 && (
          <span
            className="step-node__tool-chain"
            title={`in ordine di preferenza: ${[id, ...fallbacks].join(", ")}`}
          >
            se manca: {fallbacks.join(", ")}
          </span>
        )}
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
 * Il riquadro che dice **motore non dichiarato**, quando il passo non nomina
 * nessuno strumento in `with`.
 *
 * Prima, senza strumento, il riquadro semplicemente non compariva: un passo che
 * gira qui sulla macchina era indistinguibile da un passo di cui non si era
 * ancora guardato il motore. L'assenza è un fatto e va disegnata — è il vincolo
 * «chiarezza per chi guarda» nella sua forma più letterale.
 *
 * **LA PAROLA ERA «NESSUN MOTORE», E SI CONTRADDICEVA CON LA RIGA SOTTO.** Il
 * riquadro scrive sotto il nome dell'azione, e su un `external_engine` si
 * leggeva «*nessun motore*» sopra «motore esterno». Non era nemmeno quello che
 * il dato diceva: l'assenza qui è quella del **campo**, non quella del motore.
 * «motore non dichiarato» dice la stessa cosa e non litiga con la riga sotto.
 */
function NoTool({ action }: { action: string }) {
  return (
    <div className="step-node__tool" data-none>
      <div className="step-node__tool-text">
        <span className="step-node__tool-name">motore non dichiarato</span>
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
  const { step, kind, run: fromData, flowName, color: flowColor, dimmed, ports } =
    data as StepNodeData;
  // I FATTI VERI VINCONO SULL'ESEMPIO. `run` nei `data` esiste ancora perché è
  // così che i dati d'esempio colorano la tela fuori dal guscio nativo; quando
  // una corsa vera esiste, è la sua a contare.
  const key = nodeId(flowName, step.id);
  const runs = useContext(StepRunContext);
  const run = runs.get(key) ?? fromData;
  const usage = useContext(StepUsageContext).get(key);
  const state: StepState = run?.state ?? "waiting";
  const isAgent = kind === "engine";
  // Quale strumento esegue questo nodo si legge sulla tela, non solo aprendo il
  // pannello: è la prima domanda di chi guarda un flusso fatto da altri.
  const engines = enginesOf(step.with);
  // Quanto è lontano l'occhio. Sotto la soglia il nodo lascia cadere il
  // dettaglio invece di disegnarlo alto sei pixel.
  const zoom = useStore((s) => s.transform[2]);
  const far = zoom < FAR_ZOOM;
  // L'unico isolato della vista, e il conto di chi aspetta altrove.
  const call = stepThatCallsForAGesture(runs);
  const isolated = call.key === key;

  return (
    <div
      className="step-node"
      data-agent={isAgent || undefined}
      data-dimmed={dimmed || undefined}
      data-state={state}
      data-selected={selected || undefined}
      data-far={far || undefined}
      data-calls={isolated || undefined}
    >
      {/* Il bollino di corsia: a quale flusso appartiene questo passo, nella
          tela dove tutti i flussi stanno insieme. */}
      <div className="step-node__flow" style={{ background: flowColor }} title={flowName} />
      <Handle type="target" position={Position.Left} />

      {/* LA TESTATA INCISA dice le due cose che si chiedono per prime: che cosa
          sei, e come stai. Resta anche da lontano — è ciò che a quello zoom si
          legge ancora, e prima da lontano spariva insieme al resto. */}
      <div className="step-node__head">
        <span className="step-node__kind">{KIND_LABEL[kind]}</span>
        {step.when && <span className="step-node__when">condizionato</span>}
        {/* PUNTO PIÙ PAROLA, MAI IL SOLO COLORE (divieto 5). Il punto è il
            registro quieto — respira, uguale su ogni corsa viva — e la parola
            è ciò che regge in scala di grigi. */}
        <span className="step-node__state">
          <span className="step-node__state-dot" aria-hidden="true" />
          {STATE_LABEL[state]}
        </span>
      </div>

      <div className="step-node__body">
        <div className="step-node__id">{step.id}</div>

        {/* DA VICINO IL NODO DICE SEMPRE CON COSA GIRA — anche quando la
            risposta è «con niente». Da lontano no: a quello zoom non si
            leggerebbe. */}
        {!far && (engines.length > 0 ? (
          <StepTool
            id={engines[0]}
            model={modelOf(step)}
            actual={usage?.models ?? []}
            fallbacks={engines.slice(1)}
          />
        ) : (
          <NoTool action={step.action} />
        ))}

        {!far && usage && usage.calls > 0 && <StepMeter usage={usage} />}

        {!far && ports && <StepPortsRow ports={ports} />}
      </div>

      {!far && (run || isolated) && (
        <div className="step-node__foot">
          {/* Chi tiene il passo è un fatto, e sta bene in fondo: non chiede
              niente a nessuno. Prima viaggiava dentro il riquadro che urlava. */}
          {isAgent && state === "running" && (
            <span className="step-node__pid">pid {run?.held_by_pid ?? "?"}</span>
          )}
          {run && run.attempt > 1 && (
            <span className="step-node__attempt">
              {run.attempt}ª di {step.max_attempts}
            </span>
          )}
          {/* Le altre che aspettano non prendono ciascuna un'evidenza: si
              contano qui, sull'unico nodo isolato. */}
          {isolated && call.waiting > 1 && (
            <span className="step-node__elsewhere">
              altri {call.waiting - 1} in attesa
            </span>
          )}
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
