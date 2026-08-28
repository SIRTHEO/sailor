import type { Edge, Node } from "@xyflow/react";
import type { Graph, StepRun } from "./flow";
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
