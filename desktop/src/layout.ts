import type { Edge, Node } from "@xyflow/react";
import type { FlowFile, Graph, StepRun } from "./flow";
import { kindOf } from "./flow";

const COLUMN = 260;
const ROW = 150;

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
 * `layout.test.ts` rifà il conto leggendo `styles.css`: se un corpo o
 * un'interlinea di `.flow-band__*` cambia e questo numero resta fermo, diventa
 * rosso. Il conto è: padding in alto + la riga più alta dell'intestazione +
 * lo stacco della descrizione + due righe di descrizione + `BAND_HEAD_GAP`.
 */
export const BAND_PAD_TOP = 88;

/** Quanto respiro deve restare fra la descrizione e il primo nodo. */
export const BAND_HEAD_GAP = 8;

/** La descrizione di una corsia è tagliata a due righe da `styles.css`. */
export const BAND_DESC_LINES = 2;

const BAND_PAD_BOTTOM = 24;
const STEP_WIDTH = 232;

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
    const height = maxRows * ROW + BAND_PAD_TOP + BAND_PAD_BOTTOM;

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
