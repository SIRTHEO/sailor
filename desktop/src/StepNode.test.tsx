// @vitest-environment jsdom
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { Step, StepRun } from "./flow";
import { StepNode, StepRunContext, type StepNodeData } from "./StepNode";

/**
 * **IL NODO MOSTRA LO STATO VERO, NON QUELLO DELL'ESEMPIO.**
 *
 * Questa prova esiste perché il difetto che ha chiuso stava proprio qui e nessun
 * tipo lo vedeva: la tela compilava, i nodi si disegnavano, e dicevano «in
 * attesa» su ogni passo di ogni flusso vero anche mentre il motore lavorava.
 * Le prove su `stepStatesOfCanvas` provano il calcolo; questa prova il
 * **collegamento**, che è il punto dove si era rotto.
 */

// Ogni prova monta il proprio nodo: senza questo restano tutti attaccati e
// «trovato piu di un elemento» nasconde il vero esito.
afterEach(cleanup);

const STEP: Step = {
  id: "implementa",
  action: "external_engine",
  deps: [],
  with: {},
  species: "repeatable",
  max_attempts: 1,
} as unknown as Step;

function mountNode(data: Partial<StepNodeData>, states: Map<string, StepRun>) {
  const full: StepNodeData = {
    step: STEP,
    kind: "engine",
    flowName: "sviluppa-sailor",
    color: "#000",
    dimmed: false,
    ...data,
  };
  // Le proprietà che React Flow passa a un nodo sono molte e nessuna di quelle
  // che mancano conta qui: si dichiara il taglio invece di riempirle di finti.
  const props = {
    id: "n",
    type: "step",
    data: full,
    selected: false,
    zIndex: 0,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
  } as unknown as NodeProps;

  render(
    <StepRunContext.Provider value={states}>
      <ReactFlowProvider>
        <StepNode {...props} />
      </ReactFlowProvider>
    </StepRunContext.Provider>,
  );
}

function runIn(state: StepRun["state"]): StepRun {
  return { step_id: "implementa", state, attempt: 1 };
}

describe("il nodo e lo stato della corsa", () => {
  test("senza nessuna corsa dice «in attesa», e non inventa niente", () => {
    mountNode({}, new Map());
    expect(screen.getByText("in attesa")).toBeDefined();
  });

  test("con una corsa vera dice quello che sta succedendo, adesso", () => {
    mountNode({}, new Map([["sviluppa-sailor::implementa", runIn("running")]]));
    expect(screen.getByText("in corso")).toBeDefined();
  });

  test("LA CORSA VERA VINCE SUI DATI D'ESEMPIO PORTATI DAL NODO", () => {
    // È il difetto, nella sua forma esatta: il nodo portava nei propri `data`
    // uno stato che sui flussi veri era sempre assente, e nessuno gli passava
    // quello vero. Qui i due ci sono entrambi e vince il vero.
    mountNode(
      { run: runIn("waiting") },
      new Map([["sviluppa-sailor::implementa", runIn("broke")]]),
    );
    expect(screen.getByText("rotto, si ritenta")).toBeDefined();
  });

  test("lo stato di un passo omonimo di un ALTRO flusso non arriva qui", () => {
    // Fra i flussi veri di questa macchina `verifica`, `trigger` e `verdetto`
    // sono ripetuti: con una chiave non qualificata il nodo si colorerebbe con
    // la corsa di un flusso che non è il suo.
    mountNode({}, new Map([["un-altro-flusso::implementa", runIn("went")]]));
    expect(screen.getByText("in attesa")).toBeDefined();
  });
});
