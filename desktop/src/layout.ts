import type { Edge, Node } from "@xyflow/react";
import type { FlowFile, Graph, Step, StepRun, ValueSchema } from "./flow";
import { kindOf } from "./flow";

/**
 * Quanto è largo un nodo, e quanto spazio resta fra due nodi vicini.
 *
 * **NON SONO NUMERI DI GUSTO, E IL PRECEDENTE ERA SBAGLIATO.** I nodi erano
 * larghi 232px e le colonne distavano 260: fra un nodo e il suo vicino
 * restavano **28px**, cioè meno dello spazio che il nodo ha dentro di sé. Un
 * riquadro che sta più vicino al vicino che al proprio contenuto non si legge
 * come oggetto: si legge come una fila. Il rapporto fra lo spazio dentro
 * (8-12) e quello fra i nodi (≥48) è ciò che fa leggere un nodo come un nodo.
 *
 * `ports.test.tsx` rilegge la larghezza da `styles.css`: se il foglio e questo
 * file smettono di dire lo stesso numero, o se lo stacco scende sotto
 * `MIN_NODE_GAP`, diventa rosso. Era il difetto per cui la disposizione
 * calcolava le corsie su una larghezza che il nodo non aveva più.
 *
 * (`layout.test.tsx` non nomina mai `STEP_WIDTH`: misura l'intestazione di una
 * corsia. L'indirizzo qui era sbagliato, e un indirizzo sbagliato manda chi
 * ripara a cercare la difesa dove non c'è.)
 */
export const STEP_WIDTH = 248;

/** Lo stacco minimo fra due nodi affiancati. Sotto questo tornano una fila. */
export const MIN_NODE_GAP = 48;

export const COLUMN = STEP_WIDTH + MIN_NODE_GAP;

/**
 * Lo stacco fra due nodi impilati. Uguale a quello orizzontale: due nodi
 * vicini si staccano allo stesso modo in tutte e due le direzioni, o la
 * griglia si legge come una tabella con le colonne strette.
 */
export const ROW_GAP = MIN_NODE_GAP;

/**
 * Il passo verticale: quanto un nodo occupa, più il suo stacco.
 *
 * Cresciuto con le porte — un nodo che dichiara i propri ingressi è più alto
 * di uno che li taceva — e misurato a schermo, non stimato. `ROW - ROW_GAP`,
 * cioè **160px**, è lo spazio che un nodo ha prima di toccare quello sotto.
 *
 * **QUESTO NUMERO NON È UNA GARANZIA, ED È BENE SAPERLO PRIMA.** Un nodo non ha
 * altezza fissa: cresce con le proprie porte, col riquadro del motore, col
 * contatore dei token e — da oggi — con la testata che va a capo.
 *
 * **LE MISURE SONO DUE, E VANNO TENUTE SEPARATE**: un nodo cresce quando una
 * corsa lo tocca, perché compare il fondo (tentativo, pid, «altri N in attesa»)
 * e la testata va a capo sugli stati dal nome lungo. Confondere le due
 * condizioni fa un terzo numero che non si riproduce in nessuna delle due — ed
 * è quello che c'era scritto qui: «sette nodi su 52 oltre i 160» non è né
 * l'una né l'altra.
 *
 * Misurato in Chrome sui dieci flussi di `flows/` il 01/09/2026, da vicino
 * (zoom 1), sui 52 nodi:
 *
 *  - **a riposo**, nessuna corsa: il più alto è `pubblica`, **227px** (sette
 *    porte); **5** nodi passano i 160px, **1** passa i 208. Il più alto fra
 *    quelli che hanno davvero un vicino una riga sotto è `chiedi`, **155px**:
 *    restano **52,6px** di margine.
 *  - **con lo stato lungo**, tutti in «aspetta una persona» — la parola di
 *    stato più larga: `pubblica` sale a **239px**, `chiedi` a **198px**,
 *    `leggi` a **182px**; **19** nodi passano i 160px, sempre **1** passa i
 *    208. Il margine più stretto scende a **9,6px**, fra `chiedi` e
 *    `verdetto`.
 *
 * Nessuno si sovrappone in nessuna delle due, ma nella seconda ci mancano nove
 * pixel, e solo per la forma di questi dieci grafi: `pubblica`, il nodo da
 * 227px, sotto non ha nessuno.
 *
 * Niente lo impedisce. Il giorno in cui un nodo da 227px si trova qualcuno
 * sotto, i due si toccano, e non c'è nessun controllo che lo dica: la
 * disposizione mette le righe a passo fisso senza mai chiedere quanto sono alti
 * i nodi. La cura vera è misurare l'altezza vera e impilare su quella, non
 * gonfiare questa costante — che sprecherebbe spazio su tutti i nodi bassi per
 * il caso di uno alto.
 */
export const ROW = 208;

/**
 * Dispone i passi per livelli: un passo sta a destra di tutti quelli da cui
 * dipende. Non è un algoritmo di bellezza — è la sola disposizione in cui la
 * freccia non torna mai indietro, e chi guarda legge l'ordine senza fidarsi.
 *
 * Un ciclo qui non può esistere: `flow::Graph` lo rifiuta al caricamento. Se
 * ne arrivasse uno lo stesso, i passi irrisolti finiscono in fondo invece di
 * far girare a vuoto il calcolo.
 */
export function depths(graph: Graph): Map<string, number> {
  const known = new Map<string, number>();
  const byId = new Map(graph.steps.map((step) => [step.id, step]));
  let progressed = true;

  while (progressed && known.size < graph.steps.length) {
    progressed = false;
    for (const step of graph.steps) {
      if (known.has(step.id)) continue;
      const ready = step.deps.every((dep) => known.has(dep));
      if (!ready) continue;
      const depth = step.deps.reduce(
        (deepest, dep) => Math.max(deepest, (known.get(dep) ?? 0) + 1),
        0,
      );
      known.set(step.id, depth);
      progressed = true;
    }
  }

  // Ciò che non si è risolto: in fondo, visibile, mai nascosto.
  const last = Math.max(0, ...known.values()) + 1;
  for (const step of graph.steps) {
    if (!known.has(step.id)) known.set(step.id, last);
    void byId;
  }
  return known;
}

export function toNodes(graph: Graph, runs: Map<string, StepRun>): Node[] {
  const depth = depths(graph);
  const perColumn = new Map<number, number>();

  return graph.steps.map((step) => {
    const column = depth.get(step.id) ?? 0;
    const row = perColumn.get(column) ?? 0;
    perColumn.set(column, row + 1);

    return {
      id: step.id,
      type: "step",
      position: { x: column * COLUMN, y: row * ROW },
      data: {
        step,
        kind: kindOf(step.action),
        run: runs.get(step.id),
        ports: portsOf(graph, step, {}),
      },
    };
  });
}

export function toEdges(graph: Graph): Edge[] {
  const skippable = new Set(
    (graph.skippable_dependencies ?? []).map(
      (edge) => `${edge.step}<-${edge.dependency}`,
    ),
  );

  return graph.steps.flatMap((step) =>
    step.deps.map((dependency) => {
      // Una dipendenza saltabile si disegna tratteggiata: promette che il suo
      // dato può mancare, e chi guarda deve saperlo senza aprire il file.
      const optional = skippable.has(`${step.id}<-${dependency}`);
      return {
        id: `${dependency}->${step.id}`,
        source: dependency,
        target: step.id,
        animated: false,
        style: optional
          ? { strokeDasharray: "5 4", stroke: "#c084fc" }
          : { stroke: "#94a3b8" },
      } satisfies Edge;
    }),
  );
}

// ── la tela unica: tutti i flussi, rami dello stesso sistema ────────────
//
// Theo, 28/08: «dovrebbe essere un unico sistema con tutti i rami connessi».
// Oggi nessun flusso dichiara una dipendenza verso un passo di un altro
// flusso (nessuna azione `subflow` è mai usata sui quattordici file reali):
// disegnare un arco fra due flussi sarebbe un arco inventato. Quello che si
// può fare onestamente è mostrarli come rami dello stesso albero — una tela
// sola, ciascun flusso nella propria corsia colorata — senza cucire ponti
// che nel disco non esistono.

const BAND_GAP = 56;
const BAND_PAD_X = 28;

/**
 * L'altezza riservata all'intestazione di una corsia, prima del primo nodo.
 *
 * **È UN NUMERO CHE DIPENDE DAL FOGLIO DI STILE, E NIENTE LO SA.** Sotto
 * `FAR_ZOOM` le etichette di una corsia crescono, e lo zoom che la tela sceglie
 * da sola con due flussi è 0,5: la modalità «da lontano» **è la vista
 * d'apertura**. Quando i corpi sono passati da 13/11px a 15/15px questo numero
 * è rimasto 54, e la descrizione della corsia è finita 4px dentro il primo
 * nodo — sulla prima schermata, dove si vede per forza.
 *
 * `layout.test.tsx` rifà il conto leggendo `styles.css`: se un corpo o
 * un'interlinea di `.flow-band__*` cambia e questo numero resta fermo, diventa
 * rosso. Il conto è: padding in alto + la riga più alta dell'intestazione +
 * lo stacco della descrizione + due righe di descrizione + `BAND_HEAD_GAP`.
 */
export const BAND_PAD_TOP = 88;

/** Quanto respiro deve restare fra la descrizione e il primo nodo. */
export const BAND_HEAD_GAP = 8;

/** La descrizione di una corsia è tagliata a due righe da `styles.css`. */
export const BAND_DESC_LINES = 2;

/**
 * Il respiro in fondo a una corsia — e la tolleranza di chi sfora.
 *
 * L'ultima riga di nodi non si porta dietro il proprio stacco: senza questa
 * sottrazione una corsia con una riga sola restava alta come se ne avesse due,
 * e sotto l'unica fila di nodi c'era una fascia vuota che si vedeva a occhio.
 * Il conto qui sotto la toglie e la rimette come padding, che è quello che
 * serviva: respiro per la corsia, e spazio per un nodo più alto della norma —
 * uno che dichiara quattro ingressi cresce oltre `ROW - ROW_GAP`.
 */
const BAND_PAD_BOTTOM = 40;

/** Una tavolozza fissa: ogni flusso prende un colore per indice, ciclico. */
const FLOW_COLORS = [
  "#2563eb",
  "#16a34a",
  "#d97706",
  "#dc2626",
  "#7c3aed",
  "#0891b2",
  "#db2777",
  "#65a30d",
  "#4f46e5",
  "#ea580c",
  "#0d9488",
  "#9333ea",
  "#ca8a04",
  "#e11d48",
];

export function colorForFlow(index: number): string {
  return FLOW_COLORS[index % FLOW_COLORS.length];
}

// ── le porte di un passo ────────────────────────────────────────────────
//
// **IL TIPO STA NELLA FORMA, E IL CABLAGGIO NEL PIENO.** Tre forme — cerchio
// testo, rombo struttura, quadrato valore — e una porta è vuota quando niente
// la alimenta, piena quando qualcosa la alimenta. Così «quale ingresso manca»
// si legge dalla tela, senza aprire il pannello e senza un bollino d'errore.
//
// LE TRE FORME SONO TRE FAMIGLIE DI `ValueSchema`, NON TRE INVENZIONI. La
// bozza approvata diceva «cerchio testo, rombo struttura, quadrato file»: il
// linguaggio di schemi di `flow::ValueSchema` **non ha un tipo file**, e
// disegnarne uno vorrebbe dire promettere una distinzione che il motore non
// fa. Il terzo quadrato porta quindi gli scalari — numero, booleano, nullo, e
// ciò che il file lascia indeterminato.
//
// DA DOVE VIENE «CABLATA», parola per parola come la compone `step_input` in
// `crates/flow/src/executor.rs`:
//  - `step.with[nome]`   → il valore è scritto lì dentro, e vince su tutto;
//  - nessuna dipendenza  → arriva da `inputs[id-del-passo]` del file di flusso;
//  - una dipendenza sola non saltabile → l'ingresso **è** l'uscita di quella;
//  - più dipendenze      → un oggetto con una chiave per dipendenza.
// Chi non ricade in nessuno dei quattro casi non è alimentato da niente.
//
// **C'È UN QUINTO ALIMENTATORE, E `portsOf` NON LO SA: `workdir`.** Dopo
// `overlay_input`, `resolve_workdir` (stesso file del motore) infila la radice
// dello spazio di lavoro nell'ingresso di **ogni** passo il cui schema la
// accetta — `accepts_property` in `crates/flow/src/schema.rs`: uno schema
// `any`, uno con `allow_extra: true`, o uno che dichiara `workdir` fra le
// proprie proprietà. Un passo che dichiarasse `workdir` nel proprio
// `input_schema` senza scriverlo in `with` lo vedrebbe quindi arrivare dal
// motore, mentre qui la porta risulterebbe vuota e — se fosse obbligatoria —
// direbbe «workdir manca». Sarebbe un'accusa falsa.
//
// Oggi è un debito e non un guasto, ed è misurato: nessuno dei dieci flussi di
// `flows/` dichiara `workdir` in uno schema d'ingresso. I quattro `workdir` che
// si trovano — in `esamina-la-repo-ramificando` e `esamina-la-repo-riscoprendo`
// — stanno tutti in `with`, che è il primo dei quattro casi qui sopra. Chi
// scriverà il primo schema che dichiara `workdir` deve aggiungere qui il caso,
// prima che la tela accusi il motore di non aver fatto il proprio lavoro.

/** Le tre forme di una porta. La tinta è solo ridondanza. */
export type PortShape = "text" | "structure" | "value";

/**
 * Da dove arriva il valore di una porta.
 *
 * `unknown` non è `none`, ed è la stessa disciplina che il nodo usa già per lo
 * strumento: quando la dipendenza dichiara `any`, quali chiavi produrrà **non
 * si sa**, e dichiarare «manca» sarebbe un'accusa inventata.
 */
export type PortFeed = "fixed" | "upstream" | "flow" | "unknown" | "none";

export interface StepPort {
  /** Il nome scritto nel file: è un dato, e resta come sta. */
  name: string;
  shape: PortShape;
  /** Vero quando qualcosa alimenta davvero questa porta. */
  wired: boolean;
  /** Vero quando lo schema la dichiara obbligatoria. */
  required: boolean;
  feed: PortFeed;
}

export interface StepPorts {
  inputs: StepPort[];
  output: StepPort;
}

/** L'ingresso di un passo che non dipende da nessuno: lo apre il file di flusso. */
export const ROOT_PORT_NAME = "avvio";

/** L'unica uscita di un passo. Vuota quando nessuno la legge. */
export const OUTPUT_PORT_NAME = "uscita";

export function shapeOf(schema: ValueSchema | undefined): PortShape {
  if (schema === undefined) return "value";
  switch (schema.type) {
    case "string":
      return "text";
    case "object":
    case "array":
      return "structure";
    // Una scelta fra testi resta testo: è la forma che chi guarda riconosce.
    case "one_of":
      return schema.values.every((value) => typeof value === "string") ? "text" : "value";
    default:
      return "value";
  }
}

/** Le proprietà che uno schema dichiara, o niente se non è un oggetto. */
function objectSchema(
  schema: ValueSchema | undefined,
): { properties: Record<string, ValueSchema>; required: string[] } | null {
  if (schema === undefined || schema.type !== "object") return null;
  return { properties: schema.properties, required: schema.required };
}

function isSkippable(graph: Graph, step: string, dependency: string): boolean {
  return (graph.skippable_dependencies ?? []).some(
    (edge) => edge.step === step && edge.dependency === dependency,
  );
}

/**
 * I nomi che il motore metterà davvero nell'ingresso di questo passo, oppure
 * `null` quando la dipendenza non dichiara le proprie chiavi.
 */
function suppliedNames(
  graph: Graph,
  step: Step,
  flowInputs: Record<string, unknown>,
): Set<string> | null {
  const byId = new Map(graph.steps.map((other) => [other.id, other]));
  const deps = step.deps;
  if (deps.length === 0) {
    const opening = flowInputs[step.id];
    if (opening === null || typeof opening !== "object" || Array.isArray(opening)) {
      return new Set();
    }
    return new Set(Object.keys(opening as Record<string, unknown>));
  }
  if (deps.length === 1 && !isSkippable(graph, step.id, deps[0])) {
    const produced = objectSchema(byId.get(deps[0])?.output_schema);
    return produced === null ? null : new Set(Object.keys(produced.properties));
  }
  return new Set(deps);
}

/**
 * Le porte di un passo, lette dal grafo e dal file — mai inventate.
 *
 * Quando lo schema d'ingresso non dichiara niente (`any`, che è il caso di
 * più della metà dei passi veri) il nodo non resta muto: mostra una porta per
 * dipendenza, o la porta d'avvio se dipendenze non ne ha. Un passo senza
 * dipendenze e senza una voce in `inputs` riceve `null`, e la porta vuota lo
 * dice — è vero, ed è la cosa che non si vedeva da nessuna parte.
 */
export function portsOf(
  graph: Graph,
  step: Step,
  flowInputs: Record<string, unknown>,
): StepPorts {
  const byId = new Map(graph.steps.map((other) => [other.id, other]));
  const supplied = suppliedNames(graph, step, flowInputs);
  const declared = objectSchema(step.input_schema);
  const inputs: StepPort[] = [];

  if (declared !== null) {
    for (const [name, schema] of Object.entries(declared.properties)) {
      const fixed = step.with != null && Object.hasOwn(step.with, name);
      const feed: PortFeed = fixed
        ? "fixed"
        : supplied === null
          ? "unknown"
          : supplied.has(name)
            ? step.deps.length === 0
              ? "flow"
              : "upstream"
            : "none";
      inputs.push({
        name,
        shape: shapeOf(schema),
        wired: feed !== "none",
        required: declared.required.includes(name),
        feed,
      });
    }
  } else if (step.deps.length > 0) {
    for (const dependency of step.deps) {
      inputs.push({
        name: dependency,
        shape: shapeOf(byId.get(dependency)?.output_schema),
        wired: true,
        // Una dipendenza saltabile promette già che il suo dato può mancare.
        required: !isSkippable(graph, step.id, dependency),
        feed: "upstream",
      });
    }
  } else {
    const opened = flowInputs[step.id] !== undefined;
    inputs.push({
      name: ROOT_PORT_NAME,
      shape: shapeOf(step.input_schema),
      wired: opened,
      required: false,
      feed: opened ? "flow" : "none",
    });
  }

  const consumed = graph.steps.some((other) => other.deps.includes(step.id));
  return {
    inputs,
    output: {
      name: OUTPUT_PORT_NAME,
      shape: shapeOf(step.output_schema),
      wired: consumed,
      required: false,
      feed: consumed ? "upstream" : "none",
    },
  };
}

/**
 * Un nodo appartiene a un flusso: l'identificatore porta il nome del flusso
 * davanti, perché due passi di flussi diversi possono chiamarsi allo stesso
 * modo (`chain-brake` esiste più di una volta) e la tela è una sola.
 */
export function nodeId(flowName: string, stepId: string): string {
  return `${flowName}::${stepId}`;
}

export function splitNodeId(id: string): { flowName: string; stepId: string } {
  const separator = id.indexOf("::");
  return { flowName: id.slice(0, separator), stepId: id.slice(separator + 2) };
}

/**
 * Vero se collegare `from -> to` (cioè far dipendere `to` da `from`) chiude
 * un ciclo — cioè se `from` dipende, anche indirettamente, già da `to`.
 */
export function wouldCycle(graph: Graph, from: string, to: string): boolean {
  const byId = new Map(graph.steps.map((step) => [step.id, step]));
  const stack = [from];
  const seen = new Set<string>();
  while (stack.length > 0) {
    const current = stack.pop() as string;
    if (current === to) return true;
    if (seen.has(current)) continue;
    seen.add(current);
    const step = byId.get(current);
    if (step) stack.push(...step.deps);
  }
  return false;
}

export interface FlowBand {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
}

export interface UnifiedLayout {
  nodes: Node[];
  edges: Edge[];
  /** Il riquadro di ciascun flusso, in coordinate della tela: serve a mettere a fuoco un ramo. */
  bands: Map<string, FlowBand>;
}

/**
 * Dispone tutti i flussi su una tela sola, una corsia orizzontale per
 * flusso, impilate dall'alto in basso. Dentro una corsia vale lo stesso
 * ordine per livelli di `toNodes`/`toEdges`; fra corsie non passa nessun
 * arco, per il motivo scritto sopra.
 */
export function buildUnifiedLayout(
  flows: Array<{ name: string; flow: FlowFile }>,
  runs: Map<string, StepRun>,
  focus: string | null,
): UnifiedLayout {
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  const bands = new Map<string, FlowBand>();
  let top = 0;

  flows.forEach(({ name, flow }, index) => {
    const graph = flow.graph;
    const depth = depths(graph);
    const color = colorForFlow(index);
    const dimmed = focus !== null && focus !== name;

    let maxColumn = 0;
    const perColumn = new Map<number, number>();
    for (const step of graph.steps) {
      const column = depth.get(step.id) ?? 0;
      maxColumn = Math.max(maxColumn, column);
      perColumn.set(column, (perColumn.get(column) ?? 0) + 1);
    }
    const maxRows = Math.max(1, ...perColumn.values());
    const width = maxColumn * COLUMN + STEP_WIDTH + BAND_PAD_X * 2;
    const height = maxRows * ROW - ROW_GAP + BAND_PAD_TOP + BAND_PAD_BOTTOM;

    bands.set(name, { x: 0, y: top, width, height, color });
    nodes.push({
      id: `band::${name}`,
      type: "flowBand",
      position: { x: 0, y: top },
      draggable: false,
      selectable: false,
      zIndex: -1,
      style: { width, height },
      data: {
        name,
        description: flow.description,
        stepCount: graph.steps.length,
        color,
        dimmed,
      },
    });

    const placed = new Map<number, number>();
    for (const step of graph.steps) {
      const column = depth.get(step.id) ?? 0;
      const row = placed.get(column) ?? 0;
      placed.set(column, row + 1);
      nodes.push({
        id: nodeId(name, step.id),
        type: "step",
        position: { x: BAND_PAD_X + column * COLUMN, y: top + BAND_PAD_TOP + row * ROW },
        data: {
          step,
          kind: kindOf(step.action),
          run: runs.get(step.id),
          flowName: name,
          color,
          dimmed,
          // Le porte stanno nei `data` e non in un contesto perché dipendono
          // solo dal file — non da una corsa. Ciò che questa tela non sopporta
          // è ricostruire l'elenco dei nodi a ogni **fatto in arrivo**; il file
          // cambia solo quando qualcuno lo modifica, ed è proprio allora che le
          // porte devono cambiare.
          ports: portsOf(graph, step, flow.inputs ?? {}),
        },
      });
    }

    const skippable = new Set(
      (graph.skippable_dependencies ?? []).map((edge) => `${edge.step}<-${edge.dependency}`),
    );
    for (const step of graph.steps) {
      for (const dependency of step.deps) {
        const optional = skippable.has(`${step.id}<-${dependency}`);
        edges.push({
          id: `${nodeId(name, dependency)}->${nodeId(name, step.id)}`,
          source: nodeId(name, dependency),
          target: nodeId(name, step.id),
          animated: false,
          style: {
            stroke: optional ? "#c084fc" : color,
            strokeDasharray: optional ? "5 4" : undefined,
            opacity: dimmed ? 0.25 : 1,
          },
        });
      }
    }

    top += height + BAND_GAP;
  });

  return { nodes, edges, bands };
}
