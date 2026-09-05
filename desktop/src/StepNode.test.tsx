// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { Step, StepKind, StepRun } from "./flow";
import { t } from "./i18n";
import {
  KIND_LABEL,
  NowContext,
  STILL_SPEAKING_SECS,
  StepNode,
  StepRunContext,
  StepUsageContext,
  WireContext,
  formatElapsed,
  type StepNodeData,
} from "./StepNode";
import { STEP_WIDTH } from "./layout";
import { parseStylesheet, styleTree, type Stylesheet } from "./contrast";
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
  selected = false,
  now = 0,
) {
  const full: StepNodeData = {
    step: STEP,
    kind: "engine",
    flowName: "sviluppa-sailor",
    color: "#000",
    ...data,
  };
  // Le proprietà che React Flow passa a un nodo sono molte e nessuna di quelle
  // che mancano conta qui: si dichiara il taglio invece di riempirle di finti.
  const props = {
    id: "n",
    type: "step",
    data: full,
    selected,
    zIndex: 0,
    isConnectable: false,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
  } as unknown as NodeProps;

  const { container } = render(
    <NowContext.Provider value={now}>
    <StepRunContext.Provider value={states}>
      <StepUsageContext.Provider value={usage}>
        <ReactFlowProvider>
          <StepNode {...props} />
        </ReactFlowProvider>
      </StepUsageContext.Provider>
    </StepRunContext.Provider>
    </NowContext.Provider>,
  );
  return container.querySelector(".step-node") as HTMLElement;
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

/**
 * The words come from the catalogue, the same read the node does. Copying them
 * here would leave a test that proves its own copy, and it would fail the day
 * the window is published in English while the wiring stayed right.
 */
const SAYS = (state: StepRun["state"]) => t(`window.step.state.${state}`);

describe("il nodo e lo stato della corsa", () => {
  test("without any run it says «not run yet», never «waiting»: nothing is waited on", () => {
    mountNode({}, new Map());
    expect(screen.getByText(SAYS("idle"))).toBeDefined();
    expect(screen.queryByText(SAYS("waiting"))).toBeNull();
  });

  test("con una corsa vera dice quello che sta succedendo, adesso", () => {
    mountNode({}, new Map([["sviluppa-sailor::implementa", runIn("running")]]));
    expect(screen.getByText(SAYS("running"))).toBeDefined();
  });

  test("LA CORSA VERA VINCE SUI DATI D'ESEMPIO PORTATI DAL NODO", () => {
    // È il difetto, nella sua forma esatta: il nodo portava nei propri `data`
    // uno stato che sui flussi veri era sempre assente, e nessuno gli passava
    // quello vero. Qui i due ci sono entrambi e vince il vero.
    mountNode(
      { run: runIn("waiting") },
      new Map([["sviluppa-sailor::implementa", runIn("broke")]]),
    );
    expect(screen.getByText(SAYS("broke"))).toBeDefined();
  });

  test("lo stato di un passo omonimo di un ALTRO flusso non arriva qui", () => {
    // Fra i flussi veri di questa macchina `verifica`, `trigger` e `verdetto`
    // sono ripetuti: con una chiave non qualificata il nodo si colorerebbe con
    // la corsa di un flusso che non è il suo.
    mountNode({}, new Map([["un-altro-flusso::implementa", runIn("went")]]));
    expect(screen.getByText(SAYS("idle"))).toBeDefined();
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
    expect(screen.getByText("engine not declared")).toBeDefined();
    // E dice cosa lo esegue al posto suo.
    expect(screen.getByText("external_engine")).toBeDefined();
  });

  test("E NON SI CONTRADDICE CON LA RIGA SOTTO", () => {
    // «nessun motore» sopra `external_engine` — cioè sopra «motore esterno» —
    // è una contraddizione per chi guarda, e per giunta non è quello che il
    // dato dice: manca il campo, non il motore.
    mountNode({}, new Map());
    expect(screen.queryByText("nessun motore")).toBeNull();
  });

  test("UNA CATENA DI MOTORI SI LEGGE: il primo, e i ricambi come tali", () => {
    // **IL DIFETTO, NELLA SUA FORMA ESATTA.** `tool` è un `ToolChoice` —
    // `One(String)` oppure `Chain(Vec<String>)` — e la finestra lo leggeva solo
    // come stringa: su una catena rispondeva «niente», e il nodo disegnava
    // «nessun motore» sopra un passo che ne nominava tre. Sui dieci flussi di
    // `flows/` erano 20 passi su 25.
    mountNode(
      { step: { ...STEP, with: { tool: ["claude-code", "agy", "codex"] } } as Step },
      new Map(),
    );
    expect(screen.getByText("claude-code")).toBeDefined();
    expect(screen.getByText("if missing: agy, codex")).toBeDefined();
    expect(screen.queryByText("motore non dichiarato")).toBeNull();
  });

  test("L'IDENTIFICATIVO NON SI TRAVESTE DA NOME: stesso carattere della catena", () => {
    // `tool?.name ?? id`: finché la scoperta non ha risposto, nello slot del
    // nome c'è l'identificativo — un dato, non un nome. In prosa, `claude-code`
    // usciva in un carattere sulla prima riga del riquadro e in monospazio
    // sulla seconda: la stessa parola, due grafie, dentro la stessa cornice.
    //
    // La prova guarda il segno che porta la regola, non il carattere risolto:
    // in jsdom nessuna famiglia è davvero installata, e chiedere a
    // `getComputedStyle` quale font è in uso risponderebbe sulla macchina, non
    // sulla regola.
    const node = mountNode(
      { step: { ...STEP, with: { tool: ["claude-code", "agy", "codex"] } } as Step },
      new Map(),
    );
    const name = node.querySelector(".step-node__tool-name");
    expect(name?.textContent, "lo slot mostra l'identificativo, non un nome").toBe("claude-code");
    expect(
      name?.hasAttribute("data-raw"),
      "senza `data-raw` il foglio non può distinguere un nome da un identificativo",
    ).toBe(true);
    expect(stylesheetSource).toMatch(
      /\.step-node__tool-name\[data-raw\]\s*\{[^}]*font-family:\s*var\(--font-data\)/,
    );
  });

  test("un motore solo non si porta dietro una catena vuota", () => {
    mountNode({ step: { ...STEP, with: { tool: "codex" } } as Step }, new Map());
    expect(screen.getByText("codex")).toBeDefined();
    expect(screen.queryByText(/if missing/)).toBeNull();
  });

  test("senza nessuna chiamata non mostra un conto, invece di mostrare zeri", () => {
    mountNode({}, new Map());
    expect(screen.queryByText(/token entrati/)).toBeNull();
    expect(document.querySelector(".step-node__meter")).toBeNull();
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
    expect(screen.getByText("↑ 12.4k")).toBeDefined();
    expect(screen.getByText("↓ 840")).toBeDefined();
    expect(screen.getByText("$0.1285")).toBeDefined();
  });

  test("UN COSTO CHE NESSUNO HA DICHIARATO SI DICE, e non diventa zero", () => {
    // Codex dichiara il totale dei token e non i due lati: un `$0.0000` qui
    // sarebbe una misura inventata sulla faccia del nodo.
    mountNode(
      {},
      new Map(),
      new Map([["sviluppa-sailor::implementa", usageOf({ costMicros: null, callsWithoutCost: 1 })]]),
    );
    // The cell is a dash, and the reason is reachable on it. Zero would read
    // as «it ran for free», which is a different fact.
    const cell = document.querySelector(".step-node__bench .step-node__cell:last-child .step-node__cell-value");
    expect(cell?.textContent).toBe("—");
    expect(cell?.getAttribute("title")).toContain("none of this step's calls");
    expect(screen.queryByText(/0\.0000/)).toBeNull();
  });
});

/**
 * **NO ROW OF A NODE MAY DROP ONE OF ITS PIECES.**
 *
 * A flexible box that runs out of room does not shorten a label, it flattens it
 * to zero width — and an ellipsis needs a box at least one character wide, so
 * nothing is left behind to show that something went. It happened once to the
 * species in the head; the row that can overflow now is the foot.
 */
describe("a row gives up a line, never a fact", () => {
  let sheet: Stylesheet;

  beforeAll(() => {
    sheet = parseStylesheet(stylesheetSource);
  });

  /** Le dichiarazioni che il browser darebbe a un pezzo del nodo. */
  function declarationsOf(selector: string): Map<string, string> {
    const node = mountNode({}, new Map());
    const element = node.matches(selector) ? node : node.querySelector(selector);
    expect(element, `manca ${selector}`).not.toBeNull();
    const style = styleTree(document.documentElement, sheet).get(element as Element);
    expect(style, `manca lo stile calcolato di ${selector}`).toBeDefined();
    return (style as { declarations: Map<string, string> }).declarations;
  }

  test("IL GENERE HA UN FONDO, ed è dichiarato in `ch`", () => {
    // In px il fondo sarebbe giusto solo sulla macchina dove è stato misurato:
    // `--font-display` è una famiglia locale, e dove non c'è cambia il ripiego
    // e cambiano le larghezze. `ch` è la larghezza di un carattere del
    // carattere che sta girando davvero.
    const floor = declarationsOf(".step-node__kind").get("min-width");
    expect(`min-width del genere: ${String(floor)}`).toMatch(/: \d+(\.\d+)?ch$/);
    expect(Number.parseFloat(String(floor))).toBeGreaterThan(0);
  });

  test("THE FOOT WRAPS, because its four pieces ask for more than it has", () => {
    // Measured in Chrome on 2026-09-01, with the real faces, on a node forced
    // into its widest foot: «aspetta una persona», a pid, a third attempt and a
    // duration. Redo them by widening one node's foot in the inspector.
    const STATE = 133; // «aspetta una persona», the longest outcome word
    const PID = 65; // «pid 41822»
    const ATTEMPT = 59; // «3ª di 3»
    const ELAPSED = 72; // «2 min 14 s»

    const node = declarationsOf(".step-node");
    const foot = declarationsOf(".step-node__foot");
    const number = (value: string | undefined, what: string) => {
      const parsed = Number.parseFloat(String(value));
      expect(`${what}=${String(value)}`).toMatch(/=\d/);
      return parsed;
    };

    const borders =
      number(node.get("border"), "il filo del nodo") +
      number(node.get("border-left"), "il filo di stato del nodo");
    const padding = number(String(foot.get("padding")).split(/\s+/)[1], "il respiro del fondo");
    const gap = number(foot.get("gap"), "lo stacco fra i pezzi del fondo");
    const room = STEP_WIDTH - borders - padding * 2;
    const wanted = STATE + PID + ATTEMPT + ELAPSED + gap * 3;

    // If they fitted there would be nothing to ask for. They do not — 353
    // against 220 — so a row has to give: either it wraps, or a fact vanishes.
    expect(wanted).toBeGreaterThan(room);
    expect(
      foot.get("flex-wrap"),
      `stato, pid, tentativo e durata chiedono ${wanted}px e il fondo ne ha ${room}: senza andare a capo, uno sparisce`,
    ).toBe("wrap");
  });
});

/**
 * **THE OUTCOME AND HOW LONG IT TOOK, SIDE BY SIDE.**
 *
 * The duration is the one number that says whether a step is the slow one, and
 * the node had no room for it at all. What it must never do is print one it was
 * not given: on a canvas a plausible zero is read as a measurement.
 */
describe("how long the step took", () => {
  test("under ten seconds a tenth is the difference that matters", () => {
    expect(formatElapsed(0.2)).toBe("0.2 s");
    expect(formatElapsed(6)).toBe("6.0 s");
  });

  test("over ten seconds nobody compares tenths", () => {
    expect(formatElapsed(47)).toBe("47 s");
    expect(formatElapsed(134)).toBe("2 min 14 s");
    expect(formatElapsed(3900)).toBe("1 h 5 min");
  });

  test("the node prints the duration the run carries", () => {
    mountNode(
      {},
      new Map([
        [
          "sviluppa-sailor::implementa",
          { step_id: "implementa", state: "went", attempt: 1, elapsed_secs: 0.2 },
        ],
      ]),
    );
    expect(screen.getByText("0.2 s")).toBeDefined();
  });

  test("A DURATION NOBODY MEASURED LEAVES NO SLOT, and never becomes zero", () => {
    // `elapsed_secs` is optional and, on a real run, always absent: nothing
    // turns the two event instants into it yet. A `0.0 s` here would be a
    // measurement invented on the face of the node.
    const node = mountNode({}, new Map([["sviluppa-sailor::implementa", runIn("went")]]));
    expect(node.querySelector(".step-node__elapsed")).toBeNull();
  });
});

/**
 * **THE SPECIES IS A SHAPE BEFORE IT IS A WORD, AND NEVER INSTEAD OF IT.**
 *
 * Nine kinds all wearing the same head is the state the canvas was in: every
 * node said VERIFICA. The glyph tells them apart at a glance — but on its own a
 * glyph is colour by another name (prohibition 5), so the word stays.
 */
describe("the species of a node", () => {
  test("every kind carries its own glyph", () => {
    const drawn = new Set<string>();
    for (const kind of Object.keys(KIND_LABEL) as StepKind[]) {
      const node = mountNode({ kind }, new Map());
      const glyph = node.querySelector(".step-node__icon svg");
      expect(glyph, `«${KIND_LABEL[kind]}» has no glyph`).not.toBeNull();
      drawn.add((glyph as SVGElement).innerHTML);
      cleanup();
    }
    expect(drawn.size, "two kinds share a glyph").toBe(Object.keys(KIND_LABEL).length);
  });

  test("the glyph never travels alone: the word is beside it", () => {
    const node = mountNode({ kind: "deposit" }, new Map());
    const head = node.querySelector(".step-node__head") as HTMLElement;
    expect(head.querySelector(".step-node__icon")).not.toBeNull();
    expect(head.querySelector(".step-node__kind")?.textContent).toBe(KIND_LABEL.deposit);
  });

  test("the glyph is hidden from anyone reading the tree, because the word is not", () => {
    const node = mountNode({ kind: "deposit" }, new Map());
    expect(node.querySelector(".step-node__icon")?.getAttribute("aria-hidden")).toBe("true");
  });
});

/**
 * **WHAT SITS WHERE, AND WHY IT IS NOT DECORATION.**
 *
 * From far the node draws only its head and its foot. So the head has to carry
 * both halves of «which step is this» — the species and the name — and the foot
 * has to carry the outcome. Move the name into the body and, at the zoom the
 * window opens at, the canvas becomes a grid of nameless plates.
 */
describe("the two rows that survive the far zoom", () => {
  /* The band and the name below it: those two are the plate, and neither is
     inside the part that drops away at the far zoom. */
  test("the plate carries the species AND the name", () => {
    const node = mountNode({}, new Map());
    expect(node.querySelector(".step-node__head .step-node__kind")).not.toBeNull();
    expect(node.querySelector(".step-node__id")?.textContent).toBe("implementa");
    expect(node.querySelector(".step-node__body .step-node__id")).toBeNull();
  });

  /* What you are and how you are are the two questions asked first, so they
     share the band that survives the far zoom — and the answer is given once:
     the same word in two places is two places to read it wrong. */
  test("the head carries the outcome, once", () => {
    const node = mountNode({}, new Map());
    expect(node.querySelector(".step-node__head .step-node__state")).not.toBeNull();
    expect(node.querySelectorAll(".step-node__state").length).toBe(1);
  });
});

/**
 * THE FOUR CORNER MARKS, and why not just the ring. The selected node already
 * had a blue border, and that blue is the same blue as «running»: a hue on its
 * own is prohibition 5. Shape survives where a tint does not.
 */
describe("which node is the one being looked at", () => {
  test("a selected node carries four corner marks", () => {
    const node = mountNode({}, new Map(), new Map(), true);
    expect(node.querySelectorAll(".step-node__marks").length).toBe(2);
    expect(node.getAttribute("data-selected")).toBe("true");
  });

  test("a node nobody picked carries none", () => {
    const node = mountNode({}, new Map(), new Map(), false);
    expect(node.querySelectorAll(".step-node__marks").length).toBe(0);
  });
});

/**
 * WHAT COULD FOLLOW THIS, WITHOUT A DRAG. Pointing beats dragging on both time
 * and errors, and a gesture reachable only by dragging fails WCAG 2.2 SC 2.5.7.
 * button opens the same menu the wire opens, so there is one loop and two ways
 * into it.
 */
describe("asking what follows a step", () => {
  function mountWith(wire: ((id: string, flow: string, at: { x: number; y: number }) => void) | null) {
    const props = {
      id: "n",
      type: "step",
      data: { step: STEP, kind: "engine", flowName: "sviluppa-sailor", color: "#000" },
      selected: false,
      zIndex: 0,
      isConnectable: false,
      positionAbsoluteX: 0,
      positionAbsoluteY: 0,
      dragging: false,
    } as unknown as NodeProps;
    const { container } = render(
      <StepRunContext.Provider value={new Map()}>
        <WireContext.Provider value={wire}>
          <ReactFlowProvider>
            <StepNode {...props} />
          </ReactFlowProvider>
        </WireContext.Provider>
      </StepRunContext.Provider>,
    );
    return container.querySelector(".step-node") as HTMLElement;
  }

  test("the button names the step it would follow, for whoever cannot see it", () => {
    const node = mountWith(() => {});
    const button = node.querySelector(".step-node__more");
    expect(button?.getAttribute("aria-label")).toContain(STEP.id);
    expect(button?.tagName).toBe("BUTTON");
  });

  /* With nobody listening the button would do nothing when pressed, which is
     worse than not offering it. */
  test("with no listener the node draws no button at all", () => {
    expect(mountWith(null).querySelector(".step-node__more")).toBeNull();
  });

  test("pressing it asks about this step, and does not select the node", () => {
    const asked: string[] = [];
    const node = mountWith((id) => asked.push(id));
    const button = node.querySelector(".step-node__more") as HTMLElement;
    fireEvent.click(button);
    expect(asked).toEqual([STEP.id]);
  });
});

/**
 * **THE BREATHING DOT SAYS «ALIVE» FOR BOTH.** An agent printing its way
 * through a repository and one stuck on something that will never answer are
 * the same node: the engine has always sent the output piece by piece, and the
 * canvas threw it away.
 */
describe("a step that is speaking", () => {
  const spoke = (state: StepRun["state"], when: number): Map<string, StepRun> =>
    new Map([["sviluppa-sailor::implementa", { step_id: "implementa", state, attempt: 1, spoke_at: when }]]);

  test("WHILE ITS OUTPUT ARRIVES THE NODE SAYS SO, and turns a ring instead of the dot", () => {
    const node = mountNode({}, spoke("running", 1_000), new Map(), false, 1_000 + STILL_SPEAKING_SECS);
    expect(node.querySelector(".speaks"), "no ring while it talks").not.toBeNull();
    expect(node.querySelector(".step-node__state-dot"), "the dot is there as well").toBeNull();
    expect(node.textContent).toContain(t("window.step.speaking"));
  });

  test("AFTER THE SILENCE IT IS RUNNING AGAIN, not speaking for ever", () => {
    const node = mountNode({}, spoke("running", 1_000), new Map(), false, 1_000 + STILL_SPEAKING_SECS + 1);
    expect(node.querySelector(".speaks"), "the ring turns on a step gone quiet").toBeNull();
    expect(node.textContent).toContain(t("window.step.state.running"));
  });

  test("SPEAKING IS SOMETHING A RUNNING STEP DOES: a closed one keeps its ending", () => {
    // The same instant, the same clock — only the state differs. A node that
    // read the instant alone would call a finished step a talking one.
    const node = mountNode({}, spoke("went", 1_000), new Map(), false, 1_000);
    expect(node.querySelector(".speaks")).toBeNull();
    expect(node.textContent).toContain(t("window.step.state.went"));
  });
});

/**
 * **A REQUEST IS NOT AN ENDING.** Six states are things that happened; this one
 * is something asked of you, and it is the only one nothing will unblock on its
 * own. It carries a shape as well as a tint and a word, so that it can be found
 * across a screen without reading it.
 */
describe("the state that waits for a person", () => {
  test("IT WEARS A HAND, and no other state does", () => {
    const handed = mountNode({}, new Map([["sviluppa-sailor::implementa", runIn("handed_to_human")]]));
    expect(handed.querySelector(".step-node__hand"), "no hand where a person is waited on").not.toBeNull();
    // And the dot is gone, not hidden behind it: two marks for one state is a
    // second sign that means nothing.
    expect(handed.querySelector(".step-node__state-dot")).toBeNull();
    expect(handed.textContent).toContain(SAYS("handed_to_human"));

    for (const state of ["running", "went", "broke", "capped", "waiting"] as const) {
      const other = mountNode({}, new Map([["sviluppa-sailor::implementa", runIn(state)]]));
      expect(other.querySelector(".step-node__hand"), `«${state}» wears the hand too`).toBeNull();
    }
  });
});

/**
 * **THE PHASE IS SHOWN AS WRITTEN, AND ONLY WHEN WRITTEN.** It is the flow
 * author's word for where in the process a step sits; the node repeats it over
 * the name so a person can see, at a glance, which moment a run is at.
 */
describe("the phase a step names", () => {
  test("a step with a phase wears it above its name", () => {
    const node = mountNode({ step: { ...STEP, phase: "build" } }, new Map());
    const label = node.querySelector(".step-node__phase");
    const name = node.querySelector(".step-node__id");
    expect(label, "no phase label on a step that names one").not.toBeNull();
    expect(label?.textContent).toBe("build");
    expect(label?.getAttribute("title")).toBe(t("window.step.phase"));
    // Above, in the document's own order: a label under the name is a caption.
    expect(
      (label as Element).compareDocumentPosition(name as Element) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
  });

  test("a step without a phase wears no label at all", () => {
    const node = mountNode({}, new Map());
    expect(node.querySelector(".step-node__phase")).toBeNull();
  });
});
