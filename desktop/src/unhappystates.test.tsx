// @vitest-environment jsdom
import type { FunctionComponent } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";

/**
 * **GLI STATI CHE NON SONO QUELLO FELICE.**
 *
 * Un cruscotto che sta bene solo nello stato popolato è incompiuto. Qui stanno
 * le regole dei tre stati che non lo sono — la tela vuota, la lettura in corso,
 * il flusso che non si carica — e dei due difetti che li rendevano
 * indistinguibili da un guasto.
 *
 * **IN JSDOM NON C'È NIENTE DI MISURABILE**, e una prova che leggesse pixel qui
 * sarebbe verde per non aver guardato niente. Quello che si custodisce qui è la
 * **regola**: chi osserva cosa, chi scatta quando, chi dichiara cosa. I pixel
 * si guardano in un Chrome vero, e il come sta nel commento sotto
 * «l'inquadratura».
 */

/* ── la spia sull'inquadratura ──────────────────────────────────────────────
   `fitView` non lascia traccia in jsdom: il riquadro di React Flow misura zero
   e la tela non si muove comunque. L'unico modo di sapere se qualcuno l'ha
   chiesta è intercettare l'istanza che React Flow consegna con `onInit`. Qui
   passa la ReactFlow vera: si aggiunge solo un giro di boa sull'istanza. */

const spy = vi.hoisted(() => ({ fits: [] as unknown[], ready: false }));

interface Instance {
  fitView: (options?: unknown) => unknown;
}

vi.mock("@xyflow/react", async (importOriginal) => {
  const real = await importOriginal<typeof import("@xyflow/react")>();
  const { createElement: element } = await import("react");
  function WatchedReactFlow(props: { onInit?: (instance: Instance) => void }) {
    return element(real.ReactFlow as unknown as FunctionComponent<Record<string, unknown>>, {
      ...props,
      onInit: (instance: Instance) => {
        const fit = instance.fitView.bind(instance);
        instance.fitView = (options?: unknown) => {
          spy.fits.push(options ?? {});
          return fit(options);
        };
        props.onInit?.(instance);
        spy.ready = true;
      },
    });
  }
  return { ...real, ReactFlow: WatchedReactFlow };
});

/* ── il disco senza flussi ──────────────────────────────────────────────────
   La tela vuota non è un componente: è **uno schermo**. Disegnare `BlankCanvas`
   da solo lascia fuori proprio ciò che le sta accanto — il pannello destro, la
   minimappa, la barra — e sono quelli che parlavano di flussi su uno schermo
   che non ne ha.

   Fuori dal guscio i flussi vengono dall'esempio, e `App` lo legge a ogni
   montaggio. Qui l'esempio si può togliere per un turno, e lo schermo intero
   diventa misurabile senza fingere un motore. */

const disk = vi.hoisted(() => ({ empty: false, brokenOnly: false }));

vi.mock("./sample", async (importOriginal) => {
  const real = await importOriginal<typeof import("./sample")>();
  return {
    ...real,
    get SAMPLE() {
      if (disk.empty) return [];
      // Un disco con dei file che non si caricano e nessuno che si carica: è la
      // scena in cui la colonna non ha flussi da elencare ma ha da mostrare
      // perché non ce ne sono, e la scheda vuota la nomina.
      if (disk.brokenOnly) return real.SAMPLE.filter((entry) => entry.state === "broken");
      return real.SAMPLE;
    },
  };
});

import App from "./App";
import { BlankCanvas } from "./BlankCanvas";
import {
  belowThreshold,
  contrastPairs,
  parseStylesheet,
  styleTree,
  type ElementStyle,
  type Stylesheet,
} from "./contrast";
import { stepCountLabel } from "./flow";
import { SAMPLE } from "./sample";
import stylesheetSource from "./styles.css?raw";

/* ── l'osservatore del riquadro, guidato dalla prova ────────────────────────
   Negli altri file `ResizeObserver` è un guscio vuoto: basta perché React Flow
   si monti. Qui serve di più — è **l'osservatore il soggetto della regola** —
   quindi si registra chi guarda cosa e la prova decide quando il riquadro
   diventa misurabile. */

interface Watcher {
  element: Element;
  announce: (width: number, height: number) => void;
}

const watchers: Watcher[] = [];

class RecordingResizeObserver {
  private readonly heard: (entries: Array<{ target: Element; contentRect: DOMRectReadOnly }>) => void;
  private readonly mine: Watcher[] = [];

  constructor(callback: (entries: Array<{ target: Element; contentRect: DOMRectReadOnly }>) => void) {
    this.heard = callback;
  }

  observe(element: Element) {
    const watcher: Watcher = {
      element,
      announce: (width, height) =>
        this.heard([{ target: element, contentRect: { width, height } as DOMRectReadOnly }]),
    };
    this.mine.push(watcher);
    watchers.push(watcher);
  }

  unobserve(element: Element) {
    for (const watcher of this.mine.filter((candidate) => candidate.element === element)) {
      this.forget(watcher);
    }
  }

  // SMETTERE DI GUARDARE DEVE SMETTERE DAVVERO. Con un `disconnect` che non
  // toglie niente, «si inquadra una volta sola» sarebbe rosso su un codice
  // giusto: il finto continuerebbe a parlare a chi ha chiuso la linea.
  disconnect() {
    for (const watcher of [...this.mine]) this.forget(watcher);
  }

  private forget(watcher: Watcher) {
    const here = watchers.indexOf(watcher);
    if (here >= 0) watchers.splice(here, 1);
    const there = this.mine.indexOf(watcher);
    if (there >= 0) this.mine.splice(there, 1);
  }
}

/**
 * Dice a chi osserva questo elemento che il riquadro misura tanto, e torna
 * **quanti** l'hanno sentito.
 *
 * Il numero non è un di più: senza, «non si è inquadrato niente» sarebbe vero
 * anche quando nessuno sta guardando, cioè proprio nel caso che questa prova
 * deve rifiutare.
 */
function announce(element: Element, width: number, height: number): number {
  const listening = watchers.filter((watcher) => watcher.element === element);
  for (const watcher of listening) watcher.announce(width, height);
  return listening.length;
}

let sheet: Stylesheet;

beforeAll(() => {
  sheet = parseStylesheet(stylesheetSource);
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RecordingResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

beforeEach(async () => {
  // React Flow consegna l'istanza **un giro dopo** il montaggio, da dentro un
  // `setTimeout`. Qui si lascia cadere quella del turno precedente prima di
  // azzerare: se no la prova dopo crederebbe pronta un'istanza che non è la sua.
  await new Promise((resolve) => setTimeout(resolve, 5));
  watchers.length = 0;
  spy.fits.length = 0;
  spy.ready = false;
  disk.empty = false;
  disk.brokenOnly = false;
});

afterEach(cleanup);

/* ── la forma si vede, o non è una forma ────────────────────────────────────
   **CONTARE ELEMENTI NEL DOM LASCIA PASSARE UNO SCHELETRO NASCOSTO.** Il DOM è
   una porta sola: l'altra è il foglio. `width: 0` sulle targhe dei passi e le
   sette targhe spariscono; `display: none` sui gesti e la tela vuota torna la
   constatazione che questo file accusa. In tutti e due i casi il conteggio
   resta identico e la batteria resta verde.

   `styleTree` sa già rispondere: calcola `hidden` con l'eredità e tiene le
   dichiarazioni che vincono. Qui si guardano **le forme con cui una regola
   toglie di mezzo un elemento senza cancellarlo** — nascosto, trasparente,
   schiacciato, ridotto a zero — più quella con cui lo cancella. */

/** `0`, `0px`, `0%`: le forme in cui una misura è nessuna misura. */
const NO_SIZE = /^0(\.0+)?[a-z%]*$/i;

/**
 * Le proprietà con cui una regola riduce a niente un elemento. `min-width: 0`
 * non è fra queste: è un modo di dire dei flex e non nasconde nessuno — un
 * `min-*` alza un pavimento, non porta niente a zero.
 */
const SIZES = ["width", "height", "max-width", "max-height"];

/** Un'altezza fissa è un numero con un'unità: `auto`, `100%`, `fit-content` no. */
const FIXED_LENGTH = /^-?\d*\.?\d+(px|rem|em|ch|ex|vh|vw|vmin|vmax|pt|pc|cm|mm|in|q)$/i;

/** Le due parole con cui una scatola porta via ciò che non ci sta dentro. */
const CLIPPING = ["hidden", "clip"];

/**
 * Cosa toglie di mezzo questo elemento **senza scrivergli niente addosso**: il
 * ritaglio, il corpo del testo che si eredita, la scatola che lo contiene.
 *
 * `font-size` si eredita, quindi vince la prima dichiarazione che si incontra
 * risalendo; `overflow` e l'altezza no, e vanno cercati su ogni scatola fino
 * alla radice misurata.
 */
function whatComesFromTheBox(
  element: Element,
  root: Element,
  styles: Map<Element, ElementStyle>,
): string[] {
  const faults: string[] = [];
  let fontSize: string | undefined;
  let box: Element | null = element;
  while (box !== null) {
    const declarations = styles.get(box)?.declarations;
    if (declarations !== undefined) {
      const path = declarations.get("clip-path");
      if (path !== undefined && path !== "none") faults.push(`ritagliato via (clip-path: ${path})`);

      // La gemella d'antan di `clip-path`, la ricetta per soli lettori di
      // schermo. Morde **solo su una scatola posizionata**: dichiararla altrove
      // non toglie di mezzo nessuno, e chiamarlo guasto sarebbe un falso.
      const clip = declarations.get("clip");
      const edges = clip?.match(/^rect\((.*)\)$/i)?.[1].split(/[,\s]+/) ?? [];
      const positioned = ["absolute", "fixed"].includes(declarations.get("position") ?? "");
      if (positioned && edges.length === 4 && edges.every((edge) => NO_SIZE.test(edge))) {
        faults.push(`ritagliato via (clip: ${clip})`);
      }

      fontSize ??= declarations.get("font-size");

      const overflow = `${declarations.get("overflow") ?? ""} ${declarations.get("overflow-y") ?? ""}`;
      const height = declarations.get("height") ?? declarations.get("max-height");
      const cut = overflow.split(/\s+/).some((word) => CLIPPING.includes(word));
      if (cut && height !== undefined && FIXED_LENGTH.test(height)) {
        faults.push(`dentro una scatola alta ${height} che taglia il resto`);
      }
    }
    if (box === root) break;
    box = box.parentElement;
  }
  if (fontSize !== undefined && NO_SIZE.test(fontSize)) faults.push("corpo del testo a zero");
  return faults;
}

/**
 * `scale(0)`, `scaleY(0)`, `scale(1, 0)`: schiacciato è invisibile quanto
 * nascosto. E lo stesso vale per `scale: 0`, che è la stessa cosa **fuori** da
 * `transform`: una proprietà per conto suo, con i fattori separati da spazi.
 */
function flattened(declarations: Map<string, string>): boolean {
  const factors = declarations.get("scale");
  if (factors !== undefined && factors.split(/\s+/).some((factor) => NO_SIZE.test(factor))) return true;
  const transform = declarations.get("transform");
  if (transform === undefined) return false;
  for (const call of transform.matchAll(/\bscale(?:3d|x|y|z)?\(([^)]*)\)/gi)) {
    if (call[1].split(",").some((factor) => NO_SIZE.test(factor.trim()))) return true;
  }
  return false;
}

/**
 * Cosa impedisce di vedere questi elementi. Vuoto vuol dire che si vedono tutti.
 *
 * Un selettore che non trova abbastanza elementi è il primo guasto elencato:
 * cancellare non è un modo di essere visibili, e senza questa riga il controllo
 * sarebbe verde sullo schermo che ha perso tutto.
 */
function whatHidesThem(
  root: Element,
  sheet: Stylesheet,
  wanted: Array<[selector: string, atLeast: number]>,
): string[] {
  const styles = styleTree(root, sheet);
  const faults = new Set<string>();
  for (const [selector, atLeast] of wanted) {
    const found = Array.from(root.querySelectorAll(selector));
    if (found.length < atLeast) {
      faults.add(`${selector}: ${found.length} invece di ${atLeast}`);
      continue;
    }
    for (const element of found) {
      const style = styles.get(element);
      if (style === undefined) {
        faults.add(`${selector}: fuori dall'albero misurato`);
        continue;
      }
      if (style.hidden) faults.add(`${selector}: nascosto`);
      if (style.opacity === 0) faults.add(`${selector}: trasparente`);
      if (flattened(style.declarations)) faults.add(`${selector}: schiacciato`);
      for (const property of SIZES) {
        const value = style.declarations.get(property);
        if (value !== undefined && NO_SIZE.test(value)) faults.add(`${selector}: ${property} a zero`);
      }
      for (const fault of whatComesFromTheBox(element, root, styles)) {
        faults.add(`${selector}: ${fault}`);
      }
    }
  }
  return [...faults];
}

/** I gesti della tela vuota, e le targhe della lettura in corso. */
const GESTURES: Array<[selector: string, atLeast: number]> = [
  [".blank__gestures", 1],
  [".blank__gestures li", 3],
];

const SKELETON: Array<[selector: string, atLeast: number]> = [
  [".blank__skeleton", 1],
  [".blank__plate", 6],
  [".blank__plate--step", 7],
];

/** La finestra si apre su «Adesso»: la lavagna sta dietro un posto da scegliere. */
function goToFlows(): void {
  fireEvent.click(screen.getByRole("button", { name: /^Flussi/ }));
}

/* ═══ 1. L'INQUADRATURA ═════════════════════════════════════════════════════ */

/**
 * **LA LAVAGNA NASCEVA CON L'INQUADRATURA MISURATA A ZERO.**
 *
 * La catena è di quattro anelli, e ognuno è ragionevole da solo: la finestra si
 * apre su «Adesso»; la lavagna sta dentro `.body[hidden]`; il foglio dà a
 * quell'attributo un `display: none`; React Flow monta con `fitView` e misura
 * un riquadro **0×0**. I due `fitView` che restano scattano al cambio di
 * `focusName` o di `source` — **nessuno dei due scatta quando si preme
 * «flussi»**.
 *
 * Misurato in un Chrome vero, su una porta privata, con la finestra a 1440×900:
 * `nodesOnScreen: 0` su `nodesTotal: 12`, viewport `translate(-448, -158)
 * scale(0.5)`. Quei numeri qui non si possono rifare — in jsdom niente ha una
 * dimensione — e qui non si finge di rifarli: si custodisce la regola che li
 * produce.
 *
 * **DOPO LA RIPARAZIONE SONO 10 SU 12, E DI QUEI DIECI OTTO INTERI** — alla
 * stessa finestra di 1440×900, che è l'unica larghezza a cui questi numeri
 * valgono. Restano fuori dallo schermo i due inneschi `trigger::relay` e
 * `trigger::prima-corsa`, che stanno a `x = −288`; restano tagliati
 * `band::relay` e `relay::send-the-start`, e di quest'ultimo si vedono **6px
 * su 124**.
 *
 * **IL RESIDUO DIPENDE DALLA LARGHEZZA, E SI CHIUDE PRESTO.** Ogni numero qui
 * sotto è la stessa scena a una finestra diversa, misurata in Chromium:
 *
 *     1440 → tela  920, translate(12px, 251px)   send-the-start   6/124   10 su 12
 *     1464 → tela  944, translate(24px, 251px)   send-the-start  18/124   10 su 12
 *     1465 → tela  945                           send-the-start 18,5/124  12 su 12
 *     1704 → tela 1184, translate(144px, 251px)  send-the-start intero    12 su 12, interi
 *
 * **A 1465px i dodici nodi sono tutti a schermo; a 1704px sono tutti interi.**
 * Un numero solo, staccato dalla sua larghezza, racconterebbe un limite fisso
 * dove c'è una soglia — e 18 al posto di 6 lo racconterebbe anche più mite di
 * com'è a 1440.
 *
 * **IL RESIDUO È UN LIMITE DICHIARATO, NON UN LAVORO NON FINITO.** React Flow
 * ha un `minZoom` predefinito di **0,5**; l'inquadratura che terrebbe dentro
 * tutto vorrebbe **0,338** — 0,385 ignorando i due inneschi — perché la tela è
 * larga **920px** e la sola corsia `relay` è **2080 unità, cioè 1040px**. Il
 * fit chiede meno del minimo consentito, ottiene il minimo, e il resto esce dal
 * riquadro. **Ed è preesistente**: il fit rifatto a mano sul codice di prima dà
 * lo stesso transform, byte per byte. Questa riparazione porta l'inquadratura
 * da «mai» a «una volta per comparsa», non da 0,5 a 0,338.
 *
 * **E le soglie dicono di lasciare `minZoom` dov'è.** Costa 25px di finestra
 * vedere tutti i nodi e 264px vederli interi: abbassare il minimo cambierebbe
 * il gesto dello zoom su ogni schermo per recuperare una striscia larga meno di
 * un pollice. La mitigazione resta **la minimappa**, che sulla lavagna fa
 * esattamente questo mestiere: dice dove sta la roba che non si vede, col
 * colore dello stato di ogni passo.
 *
 * **UNA SFUMATURA SUL SECONDO MUTANTE, che questo commento farebbe credere
 * sbagliata.** Togliere la guardia `width === 0` dall'osservatore rende rossa
 * la prova qui sotto, ma **in Chrome non si vede niente**: lì la prima notifica
 * del `ResizeObserver` arriva già con un riquadro misurato, perché l'effetto
 * gira dopo che React ha tolto `hidden`. La guardia è cintura-e-bretelle, e la
 * sua prova è più severa del mondo. Non è un difetto — un'inquadratura su un
 * riquadro nullo è il guasto da cui si viene, e la guardia la vieta per
 * costruzione invece che per fortuna — ma non deve sembrare altro.
 *
 * **E IL DIFETTO NON È DI INQUADRATURA.** Uno schermo vuoto con una barra
 * sicura di sé accanto spiega quel vuoto in modo plausibile e falso: lo stato
 * rotto e lo stato vuoto diventano indistinguibili, che è il difetto peggiore
 * di questa sezione.
 */
describe("l'inquadratura, quando la lavagna compare", () => {
  test("PREMERE «FLUSSI» RIFÀ L'INQUADRATURA, appena il riquadro si può misurare", async () => {
    const { container } = render(<App />);
    await vi.waitUntil(() => spy.ready);

    // La lavagna esiste già, ma dietro un posto che non è il suo: è qui che il
    // riquadro nasce nullo, ed è la condizione che la riparazione deve reggere.
    const body = container.querySelector(".body") as HTMLElement;
    expect(body.hasAttribute("hidden"), "la lavagna non nasce nascosta").toBe(true);

    spy.fits.length = 0;
    goToFlows();
    expect(body.hasAttribute("hidden"), "premere «Flussi» non mostra la lavagna").toBe(false);

    const canvas = container.querySelector(".canvas") as HTMLElement;

    // Primo verso: finché il riquadro misura zero non si inquadra niente.
    // Inquadrare un riquadro nullo è esattamente il difetto da cui si viene.
    expect(
      announce(canvas, 0, 0),
      "nessuno sta guardando il riquadro della tela: la reinquadratura non può scattare",
    ).toBeGreaterThan(0);
    expect(spy.fits, "si è inquadrato un riquadro nullo").toHaveLength(0);

    // Secondo verso: appena il riquadro ha una misura, la vista si rifà.
    announce(canvas, 1200, 800);
    expect(
      spy.fits.length,
      "il riquadro è diventato misurabile e nessuno ha rifatto l'inquadratura",
    ).toBeGreaterThan(0);
  });

  test("e non si rifà a ogni respiro del riquadro", async () => {
    // Una reinquadratura a ogni misura riporterebbe la vista al centro mentre
    // qualcuno ridimensiona la finestra o apre un pannello: il gesto è
    // «mostrare la lavagna», non «essere larghi».
    const { container } = render(<App />);
    await vi.waitUntil(() => spy.ready);
    goToFlows();
    const canvas = container.querySelector(".canvas") as HTMLElement;

    announce(canvas, 1200, 800);
    const first = spy.fits.length;
    // Senza questa riga «non ne aggiunge» sarebbe vero anche per una vista che
    // non si inquadra mai: il caso da cui viene tutto il difetto.
    expect(first, "la prima misura non ha inquadrato niente").toBeGreaterThan(0);

    announce(canvas, 900, 700);
    expect(spy.fits.length, "ogni cambio di larghezza ricentra la tela").toBe(first);
  });
});

/* ═══ 2. IL PLURALE ═════════════════════════════════════════════════════════ */

/**
 * **«1 PASSI» LO LEGGE UNA PERSONA.** Il numero e il plurale nascevano in due
 * posti diversi — la colonna e l'intestazione della corsia — e tutti e due
 * scrivevano il plurale fisso. Adesso nascono dalla stessa riga.
 */
describe("un passo solo non è «1 passi»", () => {
  test("LA COLONNA E LA CORSIA CONTANO IN ITALIANO", () => {
    const { container } = render(<App />);
    goToFlows();

    const read = (selector: string) =>
      Array.from(container.querySelectorAll(selector)).map((node) => (node.textContent ?? "").trim());

    // `prima-corsa` è il flusso d'esempio da un passo solo: senza di lui questa
    // prova sarebbe verde per non aver incontrato il caso.
    const notes = read(".rail__note");
    expect(notes, "nessun flusso d'esempio ha un passo solo").toContain(stepCountLabel(1));
    expect(notes).not.toContain("1 passi");

    const counts = read(".flow-band__count");
    expect(counts, "la corsia non conta i suoi passi").toContain(stepCountLabel(1));
    expect(counts).not.toContain("1 passi");

    // E il plurale resta plurale: «niente 1 passi» sarebbe vero anche per una
    // riga che scrive sempre «passo».
    expect(notes).toContain(stepCountLabel(7));
  });

  test("la riga che conta sa contare", () => {
    expect(stepCountLabel(0)).toBe("0 passi");
    expect(stepCountLabel(1)).toBe("1 passo");
    expect(stepCountLabel(7)).toBe("7 passi");
  });
});

/* ═══ 3. GLI INVITI ═════════════════════════════════════════════════════════ */

/**
 * **DUE INVITI NELLO STESSO SCHERMO SI ANNULLANO.**
 *
 * È la regola che la barra invoca per far sparire sé stessa quando non ci sono
 * flussi, e che lo schermo accanto violava: il pannello destro chiedeva di
 * scegliere «un flusso a sinistra» mentre la barra chiedeva di sceglierne uno
 * «nella colonna» — lo stesso gesto, due nomi diversi per lo stesso posto. E
 * con un flusso già a fuoco, col suo nome scritto in alto, il pannello
 * continuava a invitare a metterlo a fuoco.
 *
 * Qui non si contano le parole: si conta **chi** invita.
 *
 * **E CHI PUÒ INVITARE NON SI SCEGLIE A MANO.** Questo elenco ne teneva fuori
 * la colonna, che invita da sempre col suo «+ Nuovo flusso»: la regola restava
 * verde su uno schermo che la violava, e il difetto che accusa qui sotto al
 * pannello — la stessa funzione con due nomi — stava a mezzo metro da lì.
 */
const OWNERS: Array<[selector: string, name: string]> = [
  [".rail", "la colonna"],
  [".blank__card", "la tela vuota"],
  [".toolbar__prompt", "la barra"],
  [".panel__empty", "il pannello"],
];

/**
 * Un invito chiede un gesto **su un flusso**: con un verbo, o col segno che al
 * verbo fa le veci.
 *
 * Pretendere il verbo lasciava fuori proprio il bottone più invitante dello
 * schermo, «+ Nuovo flusso», che il gesto lo dice con un `+`.
 */
function invites(text: string): boolean {
  return /fluss[oi]/i.test(text) && /\+|\b(scegli|crea|creane|nuovo|nuova)\b/i.test(text);
}

function whoInvites(container: HTMLElement): string[] {
  const found: string[] = [];
  for (const [selector, name] of OWNERS) {
    const owner = container.querySelector(selector);
    if (owner && invites(owner.textContent ?? "")) found.push(name);
  }
  return found;
}

describe("due inviti nello stesso schermo si annullano", () => {
  /**
   * **SULLA LAVAGNA POPOLATA GLI INVITI SONO ANCORA DUE, ed è aperto.** «+
   * Nuovo flusso» sta nella colonna e nella barra: stessa funzione, due posti.
   * Chiuderlo vuol dire decidere di chi è quel gesto quando i flussi ci sono, e
   * quella decisione non è di questa lavorazione. Sta scritto qui perché la
   * regola non torni verde su uno schermo che la viola: il giorno che il gesto
   * trova un padrone solo, questa riga diventa rossa e chiede di essere tolta.
   */
  test("A RIPOSO IL PANNELLO TACE, e i due che parlano sono la colonna e la barra", () => {
    const { container } = render(<App />);
    goToFlows();

    // La scena è quella giusta: ci sono flussi, nessuno è a fuoco, la barra ha
    // cambiato mestiere. Senza questa riga il conto sarebbe vero anche su uno
    // schermo dove non parla nessuno.
    expect(container.querySelector(".toolbar__prompt"), "la barra non sta invitando").not.toBeNull();
    expect(whoInvites(container)).toEqual(["la colonna", "la barra"]);
  });

  test("CON UN FLUSSO A FUOCO, IL PANNELLO NON INVITA A METTERLO A FUOCO", () => {
    const { container } = render(<App />);
    goToFlows();
    fireEvent.click(container.querySelector("button.rail__item") as HTMLElement);

    // Il flusso è davvero a fuoco, e il suo nome è scritto in alto: è ciò che
    // rende l'invito una richiesta di fare quello che è già fatto.
    const focused = container.querySelector(".focusbar__name, .focusbar__name-input");
    expect(focused, "nessun flusso è a fuoco").not.toBeNull();

    const panel = container.querySelector(".panel__empty") as HTMLElement;
    expect(panel, "il pannello non dice niente").not.toBeNull();
    expect(
      panel.textContent ?? "",
      "il pannello invita a mettere a fuoco un flusso che è già a fuoco",
    ).not.toMatch(/fluss[oi]/i);

    // E resta il suo mestiere: il passo, che è la cosa che il pannello mostra.
    expect(panel.textContent ?? "").toMatch(/passo/i);
  });

  test("SULLA TELA VUOTA INVITA LA TELA, non anche la colonna", () => {
    // **DENTRO `App`, non il solo componente.** Disegnata da sola, `BlankCanvas`
    // non ha accanto né la colonna né la barra né il pannello: i tre che
    // potrebbero invitare con lei mancano, e «uno solo» diventa vero per assenza.
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank[data-state='empty']"), "non è la tela vuota").not.toBeNull();
    expect(whoInvites(container)).toEqual(["la tela vuota"]);
  });
});

/* ═══ 3 bis. LO SCHERMO SENZA FLUSSI, PER INTERO ════════════════════════════ */

/**
 * **UNA PROMESSA SU UNA COSA CHE LÌ NON PUÒ ACCADERE.**
 *
 * Il pannello destro non aveva niente che lo interrogasse su questo schermo,
 * perché la tela vuota si provava da sola. Con zero flussi diceva «i parametri
 * di un passo compaiono qui»: non ci sono passi, non ci sono flussi, e il posto
 * dove comparirebbero non esiste ancora.
 *
 * La minimappa stava peggio: nessuno se ne accorgeva in **nessuno** dei due
 * versi, cioè era essa stessa un «si vede e non dice niente».
 */
describe("lo schermo senza flussi, per intero", () => {
  test("IL PANNELLO TACE A ZERO FLUSSI, e parla appena ce n'è uno", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelectorAll("button.rail__item"), "la colonna ha ancora dei flussi").toHaveLength(0);
    expect(
      container.querySelector(".panel__empty"),
      "il pannello promette i parametri di un passo su uno schermo senza passi",
    ).toBeNull();

    // Il verso opposto: senza, «tace» sarebbe vero anche per un pannello che
    // non parla mai, e il suo mestiere sparirebbe insieme al difetto.
    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    const panel = populated.container.querySelector(".panel__empty");
    expect(panel, "il pannello tace anche dove ha qualcosa da dire").not.toBeNull();
    expect(panel?.textContent ?? "").toMatch(/passo/i);
  });

  /**
   * **LA TERZA VIA È NON ESSERCI.** Far tacere il contenuto e lasciare in piedi
   * il contenitore lasciava una striscia di 288×818px — il 20% della finestra —
   * muta e divisa da un filo: non legge come calma, legge come una parte di
   * schermo che non ha finito di caricare. Una riga vera sarebbe peggio: era la
   * promessa su una cosa che lì non può accadere, ed è quella che è stata tolta.
   */
  test("LA COLONNA DESTRA SI CHIUDE A ZERO FLUSSI, e la tela si prende la sua larghezza", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".panel"),
      "una striscia muta accanto allo schermo che insegna il primo gesto",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".panel"),
      "la colonna è sparita anche dove ha i parametri di un passo da mostrare",
    ).toHaveLength(1);
  });

  /**
   * **ANCHE LA COLONNA SINISTRA SI CHIUDE A ZERO FLUSSI.** Non per simmetria
   * con la destra: perché il suo «+ Nuovo flusso» e il «Crea il primo flusso»
   * della scheda sono **la stessa funzione con due nomi**, a mezzo metro. E il
   * gesto non perde la sua casa, perché i due non convivono mai: al primo clic
   * il flusso nasce, la scheda se ne va, e la colonna torna col suo bottone
   * accanto alla cosa appena creata.
   */
  test("LA COLONNA SINISTRA SI CHIUDE A ZERO FLUSSI, e la tela si prende tutta la larghezza", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".rail"),
      "un elenco di niente col suo invito accanto, mentre la scheda offre lo stesso gesto",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".rail"),
      "la colonna è sparita anche dove ha i flussi da elencare",
    ).toHaveLength(1);
    expect(
      populated.container.querySelector(".rail__new"),
      "il gesto ha perso la sua casa permanente",
    ).not.toBeNull();
  });

  /**
   * **COI SOLI FLUSSI ROTTI LA COLONNA RESTA, E TACE.** La scheda dice che i
   * file che non si caricano stanno «in fondo alla colonna»: chiudere la
   * colonna anche qui renderebbe falsa quella riga, che è esattamente il
   * difetto — nominare un posto che non c'è — da cui viene tutta la schermata.
   * L'invito però resta uno solo: la colonna mostra, non chiama.
   */
  test("COI SOLI FLUSSI ROTTI LA COLONNA RESTA, e a invitare è ancora la sola tela", () => {
    disk.brokenOnly = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank[data-state='empty']"), "non è la tela vuota").not.toBeNull();
    expect(
      container.querySelectorAll(".rail__item[data-broken]"),
      "il flusso rotto è sparito insieme alla colonna",
    ).toHaveLength(1);

    // La riga della scheda nomina la colonna, e la colonna c'è: senza questa
    // riga «la colonna resta» sarebbe una scelta senza motivo.
    expect(container.querySelector(".blank__card")?.textContent ?? "").toMatch(/in fondo alla colonna/i);
    expect(whoInvites(container)).toEqual(["la tela vuota"]);
  });

  /**
   * **QUATTRO BOTTONI CHE INQUADRANO IL NULLA.** È parola per parola «un
   * riquadro che si vede e non dice niente» — il motivo per cui la minimappa
   * qui sotto sparisce — applicato ai comandi di React Flow. Il criterio è uno.
   */
  test("I COMANDI DELLA TELA SPARISCONO COI FLUSSI, e tornano con loro", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".react-flow__controls"),
      "comandi che ingrandiscono e inquadrano una tela senza niente dentro",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".react-flow__controls"),
      "i comandi sono spariti anche dalla lavagna, dove comandano qualcosa",
    ).toHaveLength(1);
  });

  test("LA MINIMAPPA SPARISCE COI FLUSSI, e torna con loro", () => {
    disk.empty = true;
    const { container } = render(<App />);
    goToFlows();
    expect(
      container.querySelectorAll(".react-flow__minimap"),
      "una mappa di niente sullo schermo che insegna il primo gesto",
    ).toHaveLength(0);

    cleanup();
    disk.empty = false;
    const populated = render(<App />);
    goToFlows();
    expect(
      populated.container.querySelectorAll(".react-flow__minimap"),
      "la minimappa è sparita anche dalla lavagna, dove è la mitigazione del limite dichiarato",
    ).toHaveLength(1);
  });
});

/* ═══ 4. I TRE STATI ════════════════════════════════════════════════════════ */

describe("i tre stati della tela senza flussi", () => {
  test("LA LETTURA IN CORSO È UNA FORMA, non una rotella e non una frase sola", () => {
    const { container } = render(<BlankCanvas state="loading" brokenCount={0} onCreate={() => {}} />);

    // Uno scheletro dice **cosa sta arrivando**: le corsie e i loro passi, al
    // posto dove compariranno. Sotto le sei targhe non è più una forma.
    expect(
      container.querySelectorAll(".blank__plate").length,
      "la lettura in corso non ha una forma",
    ).toBeGreaterThanOrEqual(6);

    // E la forma si vede. Contare elementi nel DOM è il modo esatto in cui
    // questa prova resterebbe verde su uno scheletro nascosto — e l'attributo
    // è una porta sola: il foglio è l'altra, e da lì `width: 0` sulle targhe
    // dei passi lascia due corsie identiche e vuote.
    const skeleton = container.querySelector(".blank__skeleton") as HTMLElement;
    expect(skeleton.hidden, "lo scheletro c'è ma nessuno lo vede").toBe(false);
    expect(
      whatHidesThem(document.documentElement, sheet, SKELETON),
      "lo scheletro è nel DOM e il foglio lo toglie di mezzo",
    ).toEqual([]);

    // E la parola resta: il divieto 5 non ammette uno stato che si legga solo
    // dalla forma, come non ne ammette uno che si legga solo dal colore.
    expect(container.textContent ?? "").toMatch(/motore/i);

    // Uno scheletro non invita: non c'è ancora niente da decidere.
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  test("LA TELA VUOTA INSEGNA IL PRIMO GESTO, e nomina un posto che esiste", () => {
    let created = 0;
    const { container } = render(
      <BlankCanvas state="empty" brokenCount={0} onCreate={() => (created += 1)} />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Crea il primo flusso/ }));
    expect(created, "il gesto offerto non fa niente").toBe(1);

    // I gesti sono in fila, e sono quelli veri. «La cassetta a sinistra» non
    // esiste più: la cassetta è una barra in fondo alla tela, e con zero flussi
    // non c'è affatto — un'istruzione falsa è peggio di nessuna istruzione.
    expect(container.textContent ?? "").not.toMatch(/cassetta a sinistra|colonna a sinistra/i);
    expect(container.querySelectorAll(".blank__gestures li").length).toBeGreaterThanOrEqual(3);

    // E i gesti si vedono: `display: none` sulla loro fila riporta la tela alla
    // constatazione muta che questo file accusa, senza toccare il conteggio.
    expect(
      whatHidesThem(document.documentElement, sheet, GESTURES),
      "i gesti sono nel DOM e il foglio li toglie di mezzo",
    ).toEqual([]);

    // I numeri li disegna il foglio su un elenco senza pallini: senza il ruolo
    // dichiarato, chi legge con le orecchie perde «elenco di tre».
    expect(
      container.querySelector(".blank__gestures")?.getAttribute("role"),
      "l'elenco dei gesti non si annuncia come elenco",
    ).toBe("list");
  });

  /**
   * **CHI MISURA VA MISURATO**, e un controllo che promette più di quanto
   * dimostra è il difetto che questo file accusa nella schermata. Quindi il
   * titolo dice quante forme vede, e qui sotto stanno per nome quelle che non
   * vede.
   *
   * **LE DIECI CHE VEDE**: cancellato, `display: none`, `visibility: hidden`,
   * `opacity: 0`, uno scale a zero, una misura a zero, `clip-path`, `clip:
   * rect(0,0,0,0)` su una scatola posizionata, il corpo del testo a zero, e la
   * scatola che ritaglia a un'altezza fissa. Le ultime sono entrate dopo, tutte
   * trovate in un Chrome vero mentre questa batteria restava verde. Quella con
   * `overflow` non è nemmeno un trucco da avversario — è la riga che qualcuno
   * scrive per contenere un riquadro, e si porta via il gesto 3 e il bottone
   * primario insieme.
   *
   * **LO SCALE SI CONTA UNA VOLTA E VALE DUE VOLTE.** `transform: scale(0)` e
   * `scale: 0` sono la stessa cosa scritta in due posti — la seconda è una
   * proprietà per conto suo — e in Chromium i tre `li` collassano a 0×0 in tutti
   * e due i casi. Guardare solo dentro `transform` era una porta aperta.
   *
   * **OTTO DI QUELLE CHE NON VEDE**: `content-visibility: hidden`; `position:
   * absolute` con un `left` fuori dallo schermo; uno `z-index: -1` sotto un
   * fratello opaco; `color: transparent`; `text-indent: -9999px`; `filter:
   * opacity(0)`; `transform: translateX(-9999px)`; `overflow: hidden` con
   * un'altezza in percentuale. **Otto, non tutte**: questo elenco è un campione,
   * e l'unico modo di chiuderlo sarebbe rifare un motore di resa.
   *
   * **QUELLO CHE MANCA A TUTTE E OTTO È LO STESSO, e non è una regola in più:
   * è il calcolo del foglio portato fino in fondo.** Dove un elemento finisce,
   * quanto diventa grande quando la misura è una percentuale, di che colore
   * risulta dipinto dopo un filtro, chi copre chi. In jsdom niente ha una
   * posizione, niente ha una dimensione e niente viene dipinto: il posto dove
   * si vincerebbe è una misura in un Chrome vero, non una riga qui.
   *
   * Il candidato che invece **non** sta lì — l'altezza di un pixel con
   * `overflow: hidden` — cade nella stessa regola della scatola che ritaglia, e
   * la prova qui sotto lo mostra.
   */
  test("IL CONTROLLO SULLA FORMA VEDE DIECI MODI DI SPARIRE, e otto di quelli che non vede stanno scritti sopra", () => {
    render(<BlankCanvas state="empty" brokenCount={0} onCreate={() => {}} />);

    // Col foglio vero non si lamenta di niente: un controllo che si lamenta
    // sempre non controlla, e le due prove qui sopra sarebbero verdi per caso.
    expect(whatHidesThem(document.documentElement, sheet, GESTURES)).toEqual([]);

    const tricks = [
      ".blank__gestures { display: none }",
      ".blank__gestures { visibility: hidden }",
      ".blank__gestures { opacity: 0 }",
      ".blank__gestures { transform: scale(0) }",
      ".blank__gestures { scale: 0 }",
      ".blank__gestures { position: absolute; clip: rect(0, 0, 0, 0) }",
      ".blank__gestures { max-height: 0 }",
      ".blank__gestures li { height: 0 }",
      ".blank__gestures li { transform: scale(1, 0) }",
      ".blank__gestures { clip-path: inset(100%) }",
      ".blank__gestures li { font-size: 0 }",
      ".blank__card { overflow: hidden; height: 150px }",
      ".blank__gestures li { height: 1px; overflow: hidden }",
    ];
    for (const trick of tricks) {
      const doctored = parseStylesheet(`${stylesheetSource}\n${trick}`);
      expect(whatHidesThem(document.documentElement, doctored, GESTURES), trick).not.toEqual([]);
    }

    // E le otto che restano fuori restano fuori **davvero**: se una diventasse
    // rossa, l'elenco qui sopra direbbe il falso nell'altro verso.
    const blind = [
      ".blank__gestures { content-visibility: hidden }",
      ".blank__gestures { position: absolute; left: -9999px }",
      ".blank__gestures { z-index: -1 }",
      ".blank__gestures { color: transparent }",
      ".blank__gestures li { text-indent: -9999px }",
      ".blank__gestures { filter: opacity(0) }",
      ".blank__gestures { transform: translateX(-9999px) }",
      ".blank__card { overflow: hidden; height: 10% }",
    ];
    for (const spot of blind) {
      const doctored = parseStylesheet(`${stylesheetSource}\n${spot}`);
      expect(whatHidesThem(document.documentElement, doctored, GESTURES), spot).toEqual([]);
    }

    // E cancellare non è un modo di essere visibili: su uno schermo che i gesti
    // non li ha affatto il foglio è innocente, e il controllo lo dice lo stesso.
    cleanup();
    render(<BlankCanvas state="loading" brokenCount={0} onCreate={() => {}} />);
    expect(whatHidesThem(document.documentElement, sheet, GESTURES)).not.toEqual([]);
  });

  test("UN FLUSSO CHE NON SI CARICA PORTA IL MOTIVO, parola per parola", () => {
    // La promessa già mantenuta, che però nessuno interrogava: bastava
    // riassumere il motivo, o toglierlo, e la batteria restava verde.
    const broken = SAMPLE.find((entry) => entry.state === "broken");
    expect(broken, "i dati d'esempio non hanno più un flusso rotto").toBeDefined();
    const reason = broken?.state === "broken" ? broken.broken.reason : "";

    const { container } = render(<App />);
    goToFlows();

    const marked = container.querySelector(".rail__item[data-broken]") as HTMLElement;
    expect(marked, "il flusso rotto è sparito dalla colonna invece di restare marcato").not.toBeNull();
    expect(marked.textContent ?? "").toContain(reason);
  });

  /**
   * **DUE SCHERMATE NUOVE, DUE SCENE NUOVE DA MISURARE.**
   *
   * Il divieto 6 vive in `contrast.test.tsx`, e le sue tre scene sono tutte
   * popolate: la tela vuota e la lettura in corso non ne attraversavano
   * nessuna. Disegnarle e non misurarle sarebbe stato riaprire il buco che
   * quel file è nato per chiudere.
   */
  test("LE DUE SCHERMATE NUOVE NON PORTANO NESSUNA ACCOPPIATA SOTTO 4,5:1", () => {
    // Ogni scena dichiara quante accoppiate si aspetta di aver trovato: chi
    // misura va misurato, e una scena che non trova niente passerebbe per il
    // motivo sbagliato. La lettura in corso ne ha poche di proposito — è una
    // forma, e la sua unica parola sta in fondo.
    for (const [state, atLeast] of [["empty", 6], ["loading", 3]] as const) {
      cleanup();
      render(<BlankCanvas state={state} brokenCount={2} onCreate={() => {}} />);
      const pairs = contrastPairs(document.documentElement, sheet);
      expect(pairs.length, `«${state}» non ha abbastanza testo da misurare`).toBeGreaterThanOrEqual(atLeast);
      expect(belowThreshold(pairs), `accoppiate sotto soglia in «${state}»`).toEqual([]);
    }
  });

  test("e il motore muto non offre un gesto che non si potrebbe mantenere", () => {
    // Un flusso creato mentre il motore tace non si potrebbe salvare: offrirlo
    // sarebbe una promessa che nessuno può mantenere.
    const { container } = render(
      <BlankCanvas state="failed" failure="il motore non risponde" brokenCount={0} onCreate={() => {}} />,
    );
    expect(container.querySelectorAll("button")).toHaveLength(0);
    expect(container.textContent ?? "").toContain("il motore non risponde");
  });
});
