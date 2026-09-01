// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { ReactFlowProvider, type NodeProps } from "@xyflow/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import type { Step, StepRun } from "./flow";
import { StepNode, StepRunContext, StepUsageContext, type StepNodeData } from "./StepNode";
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

  const { container } = render(
    <StepRunContext.Provider value={states}>
      <StepUsageContext.Provider value={usage}>
        <ReactFlowProvider>
          <StepNode {...props} />
        </ReactFlowProvider>
      </StepUsageContext.Provider>
    </StepRunContext.Provider>,
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
    expect(screen.getByText("motore non dichiarato")).toBeDefined();
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
    expect(screen.getByText("se manca: agy, codex")).toBeDefined();
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
    expect(screen.queryByText(/se manca/)).toBeNull();
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

/**
 * **LA TESTATA NON PUÒ CANCELLARE IL GENERE DEL NODO.**
 *
 * La testata porta due cose, e il commento che la introduce lo dichiara: *che
 * cosa sei* e *come stai*. La riparazione del traboccamento ha salvato la
 * seconda e ha cancellato la prima. `min-width: 0` più `overflow: hidden` su un
 * elemento flessibile non lo accorcia: lo **azzera**. Misurato in Chrome vero
 * sui dieci flussi di `flows/`, il 01/09/2026, con `chiedi` e `leggi` di
 * `chiedi-all-indice` in «aspetta una persona»: due nodi su 52 con
 * `clientWidth 0`, cioè `GESTO` e `AGENTE` spariti senza lasciare nemmeno
 * un'ellissi.
 *
 * I tre numeri qui sotto vengono da quella misura, presi col carattere vero:
 * si rifanno mettendo `flows/` al posto di `sample.ts`, aprendo la tela e
 * leggendo `scrollWidth` dei tre pezzi della testata.
 *
 * **PERCHÉ NON BASTA `text-overflow`.** Un'ellissi ha bisogno di una scatola
 * larga almeno un carattere; su una scatola che vale 0 non disegna niente.
 *
 * **QUALE DEI DUE PEZZI PORTA LA CURA, MISURATO SPEGNENDONE UNO ALLA VOLTA** —
 * in Chrome, sui 52 nodi, il 01/09/2026:
 *
 *  - **il wrap da solo basta**: con `min-width: 0` e la testata che va a capo,
 *    zero generi azzerati, zero troncati, zero parole di stato fuori dal nodo;
 *  - **il fondo da solo non basta**: con `nowrap` il genere sopravvive, ma il
 *    danno si sposta — la parola di stato esce di **40,5px** dal bordo del
 *    nodo (52,5px dal bordo interno della testata) su `chiedi` e `leggi`;
 *  - **senza nessuno dei due** si torna al difetto d'origine: 2 generi
 *    azzerati, 2 troncati.
 *
 * Il pezzo che porta la cura è quindi il **wrap**. Il fondo in `ch` è difesa in
 * avanti, e legittima: tiene il genere leggibile su una macchina dove
 * `--font-display` non c'è e il ripiego è più largo, dove il wrap da solo
 * comincerebbe ad accorciare. Questa prova interroga tutt'e due, e il perché è
 * diverso per ciascuno.
 */
describe("la testata dice sempre CHE COSA SEI, non solo come stai", () => {
  let sheet: Stylesheet;

  beforeAll(() => {
    sheet = parseStylesheet(stylesheetSource);
  });

  /** Le dichiarazioni che il browser darebbe a un pezzo della testata. */
  function declarationsOf(selector: string): Map<string, string> {
    const node = mountNode({}, new Map());
    const element = node.matches(selector) ? node : node.querySelector(selector);
    expect(element, `manca ${selector} nella testata`).not.toBeNull();
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

  test("QUANDO LE TRE PAROLE NON CI STANNO SU UNA RIGA, LA TESTATA VA A CAPO", () => {
    // Le tre larghezze misurate in Chrome, col carattere della testata.
    const GENDER = 40; // «AGENTE», il genere del nodo che è sparito per primo
    const WHEN = 72; // «CONDIZIONATO»
    const STATE = 133; // «ASPETTA UNA PERSONA», la parola di stato più lunga

    const node = declarationsOf(".step-node");
    const head = declarationsOf(".step-node__head");
    const number = (value: string | undefined, what: string) => {
      const parsed = Number.parseFloat(String(value));
      expect(`${what}=${String(value)}`).toMatch(/=\d/);
      return parsed;
    };

    // La larghezza utile dentro la testata: il nodo meno i suoi due fili e meno
    // il proprio respiro. Tutto letto dal foglio, niente ricopiato.
    const borders =
      number(node.get("border"), "il filo del nodo") +
      number(node.get("border-left"), "il filo di stato del nodo");
    const padding = number(String(head.get("padding")).split(/\s+/)[1], "il respiro della testata");
    const gap = number(head.get("gap"), "lo stacco fra i pezzi della testata");
    const room = STEP_WIDTH - borders - padding * 2;
    const wanted = GENDER + WHEN + STATE + gap * 2;

    // Se ci stessero, non ci sarebbe niente da chiedere. Non ci stanno — 261
    // contro 220 — ed è per questo che qualcosa deve cedere: o va a capo, o
    // sparisce, e sparire è il difetto.
    expect(wanted).toBeGreaterThan(room);
    expect(
      head.get("flex-wrap"),
      `genere, «condizionato» e stato chiedono ${wanted}px e la testata ne ha ${room}: senza andare a capo, il genere si azzera`,
    ).toBe("wrap");
  });
});
