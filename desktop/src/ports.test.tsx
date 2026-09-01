// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { FlowFile, Graph, Step, StepRun, StepState, ValueSchema } from "./flow";
import {
  COLUMN,
  MIN_NODE_GAP,
  OUTPUT_PORT_NAME,
  ROOT_PORT_NAME,
  STEP_WIDTH,
  portsOf,
} from "./layout";
import {
  StepNode,
  StepRunContext,
  StepUsageContext,
  stepThatCallsForAGesture,
  type StepNodeData,
} from "./StepNode";
import { contrastRatio, parseColor, parseStylesheet, styleTree, type Stylesheet } from "./contrast";

/**
 * **LE PORTE, E LA PROMESSA CHE PORTANO.**
 *
 * La bozza approvata dichiara tre cose sulle porte, e ognuna qui ha una riga
 * che la può far diventare rossa:
 *
 * 1. **il tipo sta nella forma** — cerchio, rombo, quadrato — e non nella
 *    tinta;
 * 2. **vuota se scollegata, piena se cablata**, così «quale ingresso manca» si
 *    legge senza aprire niente;
 * 3. e tutte e due reggono **in scala di grigi**, che è il divieto 5 in testa a
 *    `styles.css`: il colore non porta uno stato da solo.
 *
 * La terza è quella che conta, ed è misurata invece che affermata: si legge il
 * fondo che il browser darebbe al segno di una porta cablata e a quello di una
 * scollegata, e si chiede che restino lontani **in luminanza** — cioè che una
 * fotocopia in bianco e nero li tenga ancora distinti. Un disegno che li
 * distinguesse con due tinte della stessa chiarezza passerebbe l'occhio e
 * fallirebbe qui.
 */

afterEach(cleanup);

let sheet: Stylesheet;

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  sheet = parseStylesheet(stylesheetSource);
});

// ── il mondo di prova ───────────────────────────────────────────────────

const ANY: ValueSchema = { type: "any" };

function object(
  properties: Record<string, ValueSchema>,
  required: string[] = [],
): ValueSchema {
  return { type: "object", properties, required, allow_extra: false };
}

function stepOf(over: Partial<Step>): Step {
  return {
    id: "passo",
    deps: [],
    input_schema: ANY,
    output_schema: ANY,
    with: null,
    when: null,
    action: "external_engine",
    max_attempts: 1,
    ...over,
  };
}

function graphOf(steps: Step[], skippable: Graph["skippable_dependencies"] = []): Graph {
  return { steps, skippable_dependencies: skippable };
}

// ── il mondo vero: i flussi che il motore carica davvero ────────────────

/**
 * I file di `flows/`, letti così come stanno sul disco.
 *
 * Passano dal bundler e non da `node:fs` per la stessa ragione scritta in
 * `vite-env.d.ts`: leggerli con `node:fs` vorrebbe `@types/node`, una decima
 * dipendenza su un progetto che ne tiene nove. Si leggono come **testo** e si
 * decodificano qui, come il foglio di stile: nessuno schema di TypeScript si
 * mette in mezzo fra il file e ciò che il motore caricherebbe.
 *
 * Sono importati solo da questa prova, che non fa parte del grafo di
 * `main.tsx`: nel pacchetto che la finestra spedisce non entrano.
 */
function realFlows(): FlowFile[] {
  // **DUE POSTI, NON UNO.** Nove flussi stanno in `flows/`, di questo progetto;
  // `smista-il-lavoro` è **spedito dentro il binario** — sta in
  // `crates/flow/system/` e ci entra con `include_str!` — perché le regole di
  // instradamento che viaggiano col prodotto lo nominano, e su una macchina
  // appena installata la cartella `flows/` non esiste.
  //
  // Guardare un posto solo faceva scendere il censimento da 10 flussi a 9 e da
  // 20 catene a 18 il giorno in cui quel file si è spostato, senza che nessuno
  // lo volesse: la finestra disegna quel flusso come tutti gli altri, quindi
  // chi ne misura le porte deve vederlo. Due `glob` e non un `..`: la radice
  // intera porterebbe dentro `target/`.
  const files = {
    ...(import.meta.glob("../../flows/*.flow.json", {
      eager: true,
      query: "?raw",
      import: "default",
    }) as Record<string, string>),
    ...(import.meta.glob("../../crates/flow/system/*.flow.json", {
      eager: true,
      query: "?raw",
      import: "default",
    }) as Record<string, string>),
  };
  return Object.keys(files)
    .sort()
    .map((path) => JSON.parse(files[path]) as FlowFile);
}

interface PortCensus {
  total: number;
  text: number;
  structure: number;
  value: number;
  wired: number;
  empty: number;
}

/** Quante porte, di che forma e quante alimentate, su un gruppo di flussi. */
function portCensus(flows: FlowFile[]): PortCensus {
  const census: PortCensus = { total: 0, text: 0, structure: 0, value: 0, wired: 0, empty: 0 };
  for (const flow of flows) {
    for (const step of flow.graph.steps) {
      const ports = portsOf(flow.graph, step, flow.inputs ?? {});
      for (const port of [...ports.inputs, ports.output]) {
        census.total += 1;
        census[port.shape] += 1;
        if (port.wired) census.wired += 1;
        else census.empty += 1;
      }
    }
  }
  return census;
}

// ── 1. le porte si leggono dal file, e non si inventano ─────────────────

describe("da dove il nodo prende le proprie porte", () => {
  test("una proprietà che la dipendenza produce è CABLATA", () => {
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" } }, ["piano"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs).toHaveLength(1);
    expect(ports.inputs[0]).toMatchObject({ name: "piano", wired: true, feed: "upstream" });
  });

  test("una proprietà OBBLIGATORIA che nessuno alimenta è VUOTA, e lo dichiara", () => {
    // È la domanda che la bozza dice di voler chiudere: quale ingresso non è
    // collegato a niente, senza aprire il pannello.
    const upstream = stepOf({ id: "monte", output_schema: object({ altro: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ repo: { type: "string" } }, ["repo"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ name: "repo", wired: false, required: true });
  });

  test("un valore scritto in `with` alimenta la porta: vince su ciò che arriva", () => {
    // È la regola del motore, non una scelta di questa finestra: `overlay_input`
    // mette `with` sopra l'ingresso composto.
    const upstream = stepOf({ id: "monte", output_schema: object({}) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ tool: { type: "array", items: { type: "string" } } }, ["tool"]),
      with: { tool: ["claude-code"] },
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ name: "tool", wired: true, feed: "fixed" });
  });

  test("con PIÙ dipendenze l'ingresso è chiavato per dipendenza, e le porte pure", () => {
    const a = stepOf({ id: "a" });
    const b = stepOf({ id: "b" });
    const step = stepOf({
      id: "valle",
      deps: ["a", "b"],
      input_schema: object({ a: ANY, b: ANY, assente: ANY }),
    });
    const ports = portsOf(graphOf([a, b, step]), step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired])).toEqual([
      ["a", true],
      ["b", true],
      ["assente", false],
    ]);
  });

  test("UNA SOLA DIPENDENZA, MA SALTABILE: l'ingresso è chiavato, non è l'uscita di quella", () => {
    // **È L'UNICA REGOLA SOTTILE CHE QUESTA FINESTRA RICOPIA DAL MOTORE, ED ERA
    // SENZA DIFESA.** `step_input` in `crates/flow/src/executor.rs` scrive
    // `[only] if !graph.dependency_is_skippable(...)`: con una dipendenza sola
    // **non** saltabile l'ingresso *è* la sua uscita, e le chiavi del passo
    // sono le proprietà che quella produce. Ma se quella sola dipendenza è
    // saltabile la guardia non passa, si cade nel ramo `many`, e l'ingresso
    // diventa un oggetto con **una chiave per dipendenza** — chiave che manca
    // del tutto quando il passo saltato non ha prodotto niente.
    //
    // La prova che c'era sulla saltabile usava DUE dipendenze, dove il ramo è
    // già quello giusto per un'altra ragione: togliere `!isSkippable` da
    // `suppliedNames` lasciava tutta la batteria verde. Qui la dipendenza è
    // una sola, che è l'unico caso in cui quella condizione decide qualcosa.
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" }, monte: ANY }),
    });
    const graph = graphOf([upstream, step], [{ step: "valle", dependency: "monte" }]);
    const ports = portsOf(graph, step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired, port.feed])).toEqual([
      // `piano` NON arriva: sta dentro `monte`, non accanto.
      ["piano", false, "none"],
      // `monte` sì: è la chiave che il ramo `many` scrive.
      ["monte", true, "upstream"],
    ]);
  });

  test("la stessa dipendenza sola, NON saltabile, apre invece le proprie proprietà", () => {
    // Il gemello della prova qui sopra: senza di lui la coppia non dice che a
    // decidere è la saltabilità, dice solo com'è fatto un caso.
    const upstream = stepOf({ id: "monte", output_schema: object({ piano: { type: "string" } }) });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ piano: { type: "string" }, monte: ANY }),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs.map((port) => [port.name, port.wired, port.feed])).toEqual([
      ["piano", true, "upstream"],
      ["monte", false, "none"],
    ]);
  });

  test("quando la dipendenza dichiara `any` NON SI ACCUSA NESSUNO: è «non lo so»", () => {
    // Tre stati e non due, come già fa il riquadro dello strumento: dire
    // «manca» su un ingresso che forse arriva sarebbe un'accusa inventata.
    const upstream = stepOf({ id: "monte", output_schema: ANY });
    const step = stepOf({
      id: "valle",
      deps: ["monte"],
      input_schema: object({ qualcosa: { type: "string" } }, ["qualcosa"]),
    });
    const ports = portsOf(graphOf([upstream, step]), step, {});
    expect(ports.inputs[0]).toMatchObject({ wired: true, feed: "unknown" });
  });

  test("un passo senza dipendenze mostra l'AVVIO, pieno solo se il file lo apre", () => {
    const step = stepOf({ id: "radice" });
    const graph = graphOf([step]);
    expect(portsOf(graph, step, {}).inputs[0]).toMatchObject({
      name: ROOT_PORT_NAME,
      wired: false,
    });
    expect(portsOf(graph, step, { radice: { mandato: "x" } }).inputs[0]).toMatchObject({
      name: ROOT_PORT_NAME,
      wired: true,
    });
  });

  test("L'USCITA È VUOTA QUANDO NESSUNO LA LEGGE, ed è ciò che si vede sui flussi veri", () => {
    // Sui dieci flussi di questa macchina quasi ogni ingresso è riempito da
    // `with`: se il pieno/vuoto vivesse solo lì, la promessa sarebbe verde e
    // muta. Le foglie invece ci sono in ogni flusso, e si vedono subito.
    const leaf = stepOf({ id: "foglia", deps: ["monte"] });
    const upstream = stepOf({ id: "monte" });
    const graph = graphOf([upstream, leaf]);
    expect(portsOf(graph, leaf, {}).output).toMatchObject({
      name: OUTPUT_PORT_NAME,
      wired: false,
    });
    expect(portsOf(graph, upstream, {}).output).toMatchObject({ wired: true });
  });

  test("senza schema d'ingresso le porte sono le dipendenze, e la saltabile non è obbligatoria", () => {
    const a = stepOf({ id: "a", output_schema: { type: "string" } });
    const b = stepOf({ id: "b" });
    const step = stepOf({ id: "valle", deps: ["a", "b"] });
    const graph = graphOf([a, b, step], [{ step: "valle", dependency: "b" }]);
    const ports = portsOf(graph, step, {});
    expect(ports.inputs).toEqual([
      { name: "a", shape: "text", wired: true, required: true, feed: "upstream" },
      { name: "b", shape: "value", wired: true, required: false, feed: "upstream" },
    ]);
  });

  test("il tipo diventa forma: testo cerchio, struttura rombo, il resto quadrato", () => {
    const step = stepOf({
      id: "forme",
      input_schema: object({
        parola: { type: "string" },
        oggetto: object({}),
        elenco: { type: "array", items: ANY },
        numero: { type: "number" },
      }),
    });
    const shapes = portsOf(graphOf([step]), step, {}).inputs.map((port) => port.shape);
    expect(shapes).toEqual(["text", "structure", "structure", "value"]);
  });
});

// ── 2. il nodo disegnato ────────────────────────────────────────────────

const WIRED_AND_EMPTY: Step = stepOf({
  id: "implementa",
  deps: ["piano"],
  input_schema: object({ piano: { type: "string" }, repo: { type: "string" } }, ["repo"]),
  output_schema: { type: "string" },
});

const UPSTREAM: Step = stepOf({
  id: "piano",
  output_schema: object({ piano: { type: "string" } }),
});

function mountNode(over: Partial<StepNodeData> = {}, states: Map<string, StepRun> = new Map()) {
  const graph = graphOf([UPSTREAM, WIRED_AND_EMPTY]);
  const full: StepNodeData = {
    step: WIRED_AND_EMPTY,
    kind: "engine",
    flowName: "sviluppa-sailor",
    color: "#000",
    dimmed: false,
    ports: portsOf(graph, WIRED_AND_EMPTY, {}),
    ...over,
  };
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
  const { container } = render(
    <StepRunContext.Provider value={states}>
      <StepUsageContext.Provider value={new Map()}>
        <ReactFlowProvider>
          <StepNode {...props} />
        </ReactFlowProvider>
      </StepUsageContext.Provider>
    </StepRunContext.Provider>,
  );
  return container.querySelector(".step-node") as HTMLElement;
}

/** Il fondo che il browser darebbe a questo elemento, già opaco. */
function backdropOf(element: Element) {
  const styles = styleTree(document.documentElement, sheet);
  const style = styles.get(element);
  expect(style, "manca lo stile calcolato del segno di una porta").toBeDefined();
  return (style as { backdrop: Parameters<typeof contrastRatio>[0] }).backdrop;
}

describe("UNA PORTA CABLATA E UNA SCOLLEGATA SI DISTINGUONO SENZA IL COLORE", () => {
  test("i due segni restano lontani in luminanza: reggono la scala di grigi", () => {
    const node = mountNode();
    const wired = node.querySelector(".step-node__port[data-wired] .step-node__port-mark");
    const empty = node.querySelector(
      ".step-node__port:not([data-wired]) .step-node__port-mark",
    );
    expect(wired, "manca una porta cablata da misurare").not.toBeNull();
    expect(empty, "manca una porta scollegata da misurare").not.toBeNull();

    // 3:1 è la soglia dei segni non testuali: sotto, in bianco e nero, i due
    // diventano la stessa macchia. Qui il pieno è inchiostro e il vuoto è la
    // carta del nodo, quindi il margine è largo — ed è il margine a dover
    // restare, non il numero.
    const ratio = contrastRatio(
      backdropOf(wired as Element),
      backdropOf(empty as Element),
    );
    expect(ratio).toBeGreaterThanOrEqual(3);
  });

  test("il vuoto è vuoto davvero: il segno scollegato non ha nessun riempimento", () => {
    // Se qualcuno riempie tutte e due e affida la differenza alla tinta, la
    // misura sopra crolla — ma questa lo dice prima, e col nome della causa.
    const node = mountNode();
    const empty = node.querySelector(
      ".step-node__port:not([data-wired]) .step-node__port-mark",
    ) as Element;
    const styles = styleTree(document.documentElement, sheet);
    const declared = styles.get(empty)?.declarations.get("background");
    expect(parseColor(String(declared))?.a).toBe(0);
  });

  test("«manca» è una PAROLA, non una tinta", () => {
    mountNode();
    expect(screen.getByText("repo manca")).toBeDefined();
  });

  test("il tipo sta nella forma, e le tre forme sono tre disegni diversi", () => {
    const step = stepOf({
      id: "forme",
      input_schema: object({ parola: { type: "string" }, oggetto: object({}), numero: { type: "number" } }),
    });
    const node = mountNode({ step, ports: portsOf(graphOf([step]), step, {}) });
    const styles = styleTree(document.documentElement, sheet);
    const signature = (shape: string) => {
      const mark = node.querySelector(`.step-node__port-mark[data-shape="${shape}"]`);
      expect(mark, `manca il segno di forma ${shape}`).not.toBeNull();
      const declarations = styles.get(mark as Element)?.declarations as Map<string, string>;
      return [declarations.get("border-radius") ?? "", declarations.get("transform") ?? ""].join("|");
    };
    const all = [signature("text"), signature("structure"), signature("value")];
    expect(new Set(all).size, `due forme si disegnano uguali: ${all.join(" · ")}`).toBe(3);
  });
});

describe("lo stato: punto PIÙ parola", () => {
  const STATES: StepState[] = [
    "waiting",
    "running",
    "went",
    "broke",
    "capped",
    "handed_to_human",
  ];

  test("ogni stato porta un punto e una parola, e le parole sono tutte diverse", () => {
    const words = new Set<string>();
    for (const state of STATES) {
      const node = mountNode(
        {},
        new Map([["sviluppa-sailor::implementa", { step_id: "implementa", state, attempt: 1 }]]),
      );
      const label = node.querySelector(".step-node__state") as HTMLElement;
      expect(label.querySelector(".step-node__state-dot"), `${state} è senza punto`).not.toBeNull();
      const word = (label.textContent ?? "").trim();
      // Il punto da solo sarebbe colore e basta: la parola è ciò che regge in
      // scala di grigi, ed è il divieto 5 alla lettera.
      expect(word, `${state} è senza parola`).not.toBe("");
      words.add(word);
      cleanup();
    }
    expect(words.size).toBe(STATES.length);
  });
});

describe("i due registri dell'attenzione", () => {
  const run = (state: StepState): StepRun => ({ step_id: "x", state, attempt: 1 });

  test("fra tre che aspettano una persona, UNO SOLO prende l'isolamento", () => {
    const call = stepThatCallsForAGesture(
      new Map([
        ["f::c", run("handed_to_human")],
        ["f::a", run("handed_to_human")],
        ["f::b", run("capped")],
      ]),
    );
    expect(call.key).toBe("f::a");
    expect(call.waiting).toBe(3);
  });

  test("chi aspetta una persona viene prima di chi è fermo al tetto", () => {
    const call = stepThatCallsForAGesture(
      new Map([
        ["f::a", run("capped")],
        ["f::z", run("handed_to_human")],
      ]),
    );
    expect(call.key).toBe("f::z");
  });

  test("una corsa viva NON chiede attenzione: nessun isolato", () => {
    // È il difetto che la bozza nomina: ogni corsa viva chiedeva attenzione,
    // cioè nessuna la otteneva.
    expect(stepThatCallsForAGesture(new Map([["f::a", run("running")]])).key).toBeNull();
    expect(stepThatCallsForAGesture(new Map([["f::a", run("broke")]])).key).toBeNull();
  });

  test("sulla tela l'isolato è marcato, e conta le altre a parole", () => {
    const node = mountNode(
      {},
      new Map([
        ["sviluppa-sailor::implementa", run("handed_to_human")],
        ["zeta-flusso::coda", run("handed_to_human")],
      ]),
    );
    expect(node.getAttribute("data-calls")).toBe("true");
    expect(screen.getByText("altri 1 in attesa")).toBeDefined();
  });

  test("il secondo che aspetta NON è isolato", () => {
    const node = mountNode(
      {},
      new Map([
        ["altro-flusso::alfa", run("handed_to_human")],
        ["sviluppa-sailor::implementa", run("handed_to_human")],
      ]),
    );
    expect(node.getAttribute("data-calls")).toBeNull();
  });
});

// ── 3. il foglio e la disposizione dicono lo stesso numero ──────────────

describe("la larghezza di un nodo, e lo spazio fra due nodi", () => {
  test("IL FOGLIO E LA DISPOSIZIONE DICONO LO STESSO NUMERO", () => {
    // Il difetto di classe: `layout.ts` disponeva le corsie su una larghezza
    // che il nodo non aveva più. Nessun tipo lo vede, e a schermo si vede solo
    // quando due nodi si toccano.
    const rule = sheet.rules.find((candidate) => candidate.selector === ".step-node");
    expect(rule, "manca la regola `.step-node`").toBeDefined();
    const width = new Map((rule as { declarations: Array<[string, string]> }).declarations).get(
      "width",
    );
    expect(width).toBe(`${STEP_WIDTH}px`);
  });

  test("fra un nodo e il suo vicino resta lo stacco dichiarato", () => {
    // Sotto questo stacco un nodo smette di leggersi come oggetto e diventa una
    // fila: prima erano 28px, cioè meno dello spazio dentro il nodo stesso.
    expect(COLUMN - STEP_WIDTH).toBeGreaterThanOrEqual(MIN_NODE_GAP);
  });
});

// ── 4. la promessa, misurata sui flussi veri ────────────────────────────

/**
 * **LE TRE FORME SI DEVONO VEDERE SUI DATI VERI, O LA PROMESSA È UNA PAROLA.**
 *
 * Tutte le prove qui sopra costruiscono il proprio grafo: dimostrano che il
 * calcolo è giusto, non che sulla tela si veda qualcosa. E sui dati d'esempio
 * non si vedeva: gli schemi di `sample.ts` erano tutti `any`, quindi ogni porta
 * usciva quadrata e la distinzione fra cerchio, rombo e quadrato era
 * **invisibile per costruzione** — verde e muta.
 *
 * Questa prova legge i **dieci file veri** di `flows/`, quelli che il motore
 * carica, e li fa passare da `portsOf`. Non ricopia numeri: li ricalcola e
 * chiede che la promessa regga. Come li legge sta scritto una volta sola, su
 * `realFlows` qui sopra: `import.meta.glob` col bundler, non `node:fs`.
 *
 * Le soglie sono larghe apposta. Il numero esatto cambia ogni volta che
 * qualcuno tocca un flusso — e quel movimento è lavoro normale, non un difetto:
 * una prova che lo inseguisse diventerebbe rossa per il motivo sbagliato. Ciò
 * che non deve cambiare è che tutte e tre le forme e tutt'e due i pieni
 * esistano davvero, in quantità che non si possano scambiare per un caso.
 */
describe("le tre forme sui dieci flussi veri, non sull'esempio", () => {
  const flows = realFlows();

  test("i dieci file si leggono davvero, e sono dieci", () => {
    // Senza questa, tutte le soglie qui sotto potrebbero passare su zero file
    // letti — che è il modo più silenzioso di essere verdi per non aver
    // guardato niente.
    expect(flows.length).toBeGreaterThanOrEqual(10);
  });

  test("CERCHIO, ROMBO E QUADRATO CI SONO TUTTI, E NESSUNO È UN CASO ISOLATO", () => {
    const census = portCensus(flows);
    const shapes = `cerchi ${census.text}, rombi ${census.structure}, quadrati ${census.value} su ${census.total}`;
    // Il 01/09/2026 il conto era: 137 porte — 27 cerchi, 36 rombi, 74
    // quadrati; 111 cablate e 26 vuote. È scritto qui come riferimento, non
    // come soglia: la soglia è sotto, larga apposta.
    expect(census.total, `porte lette: ${shapes}`).toBeGreaterThan(100);
    // Dieci è molto sotto il conto vero di ognuna e molto sopra il rumore: se
    // `shapeOf` collassasse su una forma sola, due di questi tre andrebbero a
    // zero e la riga direbbe quale.
    expect(census.text, shapes).toBeGreaterThanOrEqual(10);
    expect(census.structure, shapes).toBeGreaterThanOrEqual(10);
    expect(census.value, shapes).toBeGreaterThanOrEqual(10);
  });

  test("VUOTO E PIENO CI SONO TUTT'E DUE: «quale ingresso manca» si vede", () => {
    // Se ogni porta risultasse cablata, la tela sarebbe leggibile e inutile:
    // la domanda che le porte esistono per chiudere non avrebbe mai risposta.
    const census = portCensus(flows);
    const fill = `cablate ${census.wired}, vuote ${census.empty} su ${census.total}`;
    expect(census.wired, fill).toBeGreaterThanOrEqual(10);
    expect(census.empty, fill).toBeGreaterThanOrEqual(10);
  });
});
