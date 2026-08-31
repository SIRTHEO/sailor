// @vitest-environment jsdom
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import type { Step, StepRun } from "./flow";
import { StepNode, StepRunContext, StepUsageContext, type StepNodeData } from "./StepNode";
import type { StepUsage } from "./stepusage";

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

function mountNode(
  data: Partial<StepNodeData>,
  states: Map<string, StepRun>,
  usage: Map<string, StepUsage> = new Map(),
) {
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
      <StepUsageContext.Provider value={usage}>
        <ReactFlowProvider>
          <StepNode {...props} />
        </ReactFlowProvider>
      </StepUsageContext.Provider>
    </StepRunContext.Provider>,
  );
}

function usageOf(over: Partial<StepUsage>): StepUsage {
  return {
    models: [],
    inputTokens: 0,
    outputTokens: 0,
    costMicros: null,
    calls: 1,
    callsWithoutCost: 0,
    ...over,
  };
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

/**
 * **UN NODO DICE SEMPRE CON COSA GIRA, E QUANTO È COSTATO.**
 *
 * È il vincolo «chiarezza per chi guarda» sulla tela: le direzioni di prodotto
 * dicevano che i nodi non mostrano il modello né cosa è entrato dentro di loro.
 * Il modello dichiarato c'era; mancavano l'assenza di motore — indistinguibile
 * da «non l'ho guardato» — e tutto ciò che la corsa aveva misurato.
 */
describe("il nodo e il motore che lo esegue", () => {
  test("UN PASSO SENZA MOTORE LO DICE, invece di lasciare un vuoto", () => {
    // Prima il riquadro non compariva affatto: un passo che gira qui sulla
    // macchina e un passo di cui nessuno ha guardato il motore si disegnavano
    // identici.
    mountNode({}, new Map());
    expect(screen.getByText("nessun motore")).toBeDefined();
    // E dice cosa lo esegue al posto suo.
    expect(screen.getByText("external_engine")).toBeDefined();
  });

  test("senza nessuna chiamata non mostra un conto, invece di mostrare zeri", () => {
    mountNode({}, new Map());
    expect(screen.queryByText(/token entrati/)).toBeNull();
    expect(screen.queryByText("costo non dichiarato")).toBeNull();
  });

  test("dopo una chiamata dice cosa è entrato, cosa è uscito e quanto è costato", () => {
    mountNode(
      {},
      new Map(),
      new Map([
        [
          "sviluppa-sailor::implementa",
          usageOf({ inputTokens: 12400, outputTokens: 840, costMicros: 128500 }),
        ],
      ]),
    );
    expect(screen.getByText("↑ 12,4k")).toBeDefined();
    expect(screen.getByText("↓ 840")).toBeDefined();
    expect(screen.getByText("0,1285 $")).toBeDefined();
  });

  test("UN COSTO CHE NESSUNO HA DICHIARATO SI DICE, e non diventa zero", () => {
    // Codex dichiara il totale dei token e non i due lati: un `0,0000 $` qui
    // sarebbe una misura inventata sulla faccia del nodo.
    mountNode(
      {},
      new Map(),
      new Map([["sviluppa-sailor::implementa", usageOf({ costMicros: null, callsWithoutCost: 1 })]]),
    );
    expect(screen.getByText("costo non dichiarato")).toBeDefined();
  });
});
