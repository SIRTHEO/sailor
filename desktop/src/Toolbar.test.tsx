// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";

import App from "./App";
import stylesheetSource from "./styles.css?raw";
import reactFlowSource from "@xyflow/react/dist/style.css?raw";
import { parseStylesheet } from "./contrast";
import { TOOL_GROUPS, TOOLBAR_KINDS, KINDS_WITH_ACTION, Toolbar } from "./Toolbar";
import { DEFAULT_ACTION_FOR_KIND, KNOWN_ACTIONS, type StepKind } from "./flow";

/**
 * La tela senza flussi si ottiene togliendo i dati di esempio, che è la stessa
 * cosa che vede chi apre Sailor la prima volta. Nessun gesto della finestra
 * porta a zero flussi in jsdom: cancellarli passa dal motore, che qui non c'è.
 */
const sample = vi.hoisted(() => ({ empty: false }));

vi.mock("./sample", async (importOriginal) => {
  const real = await importOriginal<typeof import("./sample")>();
  return {
    get SAMPLE() {
      return sample.empty ? [] : real.SAMPLE;
    },
    get SAMPLE_RUN() {
      return real.SAMPLE_RUN;
    },
  };
});

/**
 * **LA CASSETTA DEI PASSI, INTERROGATA DOVE SBAGLIEREBBE.**
 *
 * Tre cose non si vedono guardando uno screenshot fermo, e sono le tre che
 * questo lavoro promette:
 *
 * 1. che la barra stia **dentro** la tela e non scorra via con essa — a occhio
 *    una barra dentro il riquadro trasformato e una fuori sono identiche
 *    finché qualcuno non trascina la tela;
 * 2. che ogni famiglia offerta crei un passo con un'**azione che il motore
 *    conosce**, e nei due versi. Il taglio da nove famiglie a sette lo faceva
 *    già la cassetta di prima, leggendo la stessa mappa: quello che manca è
 *    qualcuno che tenga il legame: senza, un elenco scritto a mano si
 *    scollerebbe dalla mappa in silenzio;
 * 3. che senza un flusso a fuoco la barra **dica cosa manca** invece di
 *    limitarsi a spegnersi.
 */

afterEach(() => {
  cleanup();
  sample.empty = false;
});

// React Flow misura il proprio riquadro all'avvio: fuori da un browser vero non
// c'è chi lo faccia, e senza questi due la tela non si monta affatto.
class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

/** La finestra si apre su «Adesso»: la tela sta dietro un posto da scegliere. */
function goToFlows(): void {
  fireEvent.click(screen.getByRole("button", { name: /^Flussi/ }));
}

function focusAFlow(container: HTMLElement): string {
  const item = container.querySelector("button.rail__item") as HTMLElement;
  const name = item.querySelector(".rail__label")?.textContent ?? "";
  fireEvent.click(item);
  return name;
}

/* ── il corridoio, letto dal foglio ─────────────────────────────────────── */

const sheet = parseStylesheet(stylesheetSource);

/** Le dichiarazioni di una regola, l'ultima che vince a parità di selettore. */
function declarationsOf(selector: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const rule of sheet.rules) {
    if (rule.selector !== selector) continue;
    for (const [property, value] of rule.declarations) found.set(property, value);
  }
  return found;
}

/**
 * Le lunghezze dichiarate: il reticolo di `:root` e il corridoio di `.toolbar`.
 * Sono le uniche che il conto qui sotto sa leggere.
 *
 * **QUI C'ERA SCRITTA UNA GARANZIA CHE IL CODICE NON DAVA.** La riga diceva che
 * «un numero scritto a mano dentro un `calc` non finisce qui e la prova diventa
 * rossa, che è il verso giusto in cui sbagliare». Era falsa: `sumOfTerms`
 * contava le occorrenze di `var(`, non i termini, quindi un letterale in `px`
 * accanto a un `var()` passava il controllo di completezza e spariva dal
 * totale. Su `max-width` sparire stringe — verso innocuo. Su `margin-left`
 * ALLENTA: con `calc(var(--controls-reserve) + 40px)` il conto legge 44 dove il
 * browser ne dipinge 84, la batteria resta verde e a schermo la barra entra
 * nella minimappa di 23px a ogni larghezza da 1264 in giù. Un commento che
 * promette il verso giusto in cui sbagliare, mentre il codice sbaglia
 * nell'altro, è peggio di nessun commento.
 *
 * Adesso la garanzia è vera, e a renderla vera è il resto vuoto preteso in
 * fondo a `sumOfTerms`, non questa riga.
 */
const LENGTHS = new Map<string, number>();
for (const selector of [":root", ".toolbar"]) {
  for (const [property, value] of declarationsOf(selector)) {
    const pixels = /^(\d+(?:\.\d+)?)px$/.exec(value);
    if (property.startsWith("--") && pixels) LENGTHS.set(property, Number(pixels[1]));
  }
}

function lengthOf(name: string): number {
  const value = LENGTHS.get(name);
  expect(value, `«${name}» non è una lunghezza dichiarata nel foglio`).toBeDefined();
  return value as number;
}

/**
 * La somma con segno dei termini dentro un `calc`, che è la sola forma in cui
 * il corridoio si può scrivere e rileggere. Torna i **pixel fissi**; la
 * larghezza della tela ha un accumulatore suo, e `canvasTimes` dice quante
 * volte ci si aspetta di trovarla.
 *
 * **`100%` NON È RUMORE DA CANCELLARE.** Qui c'era `.replace(/100%/g, "")`, che
 * lo toglieva dal corpo *ovunque, quante volte capitava, con qualunque segno*,
 * e un commento che diceva «si semplifica». Vero per una sola occorrenza in
 * testa, falso per una seconda e falso per una sottratta — e su quella
 * semplificazione poggia tutta la regola del corridoio, che è ciò che la fa
 * valere a **ogni** larghezza invece che a quella su cui qualcuno ha guardato.
 * Due aggiramenti passavano di qui con la batteria tutta verde:
 * `max-width: calc(100% - ... + 100%)` — il browser calcola `200% - 276px`,
 * franco reale -200px a 900 e a 1000 — e
 * `margin-left: calc(var(--controls-reserve) + var(--space-2) - 100%)`, che
 * dipinge il bordo sinistro a -328px a 900 e a -868px a 1440: la barra esce
 * dalla tela sopra i comandi di zoom mentre `left >= controls` resta vera sulla
 * carta. Il mutante che questo file cattura costava 85px; questi ne costavano
 * 200, ed erano invisibili.
 *
 * Adesso il `100%` passa dallo stesso motivo degli altri termini, con segno e
 * coefficiente, e la semplificazione è una cosa che si **verifica** invece di
 * assumerla.
 */
function sumOfTerms(value: string, canvasTimes: number): number {
  const body = /^calc\((.*)\)$/.exec(value.trim())?.[1] ?? value.trim();
  const term = /(^|[+-])\s*(?:(\d+)\s*\*\s*)?(?:var\((--[a-z0-9-]+)\)|(\d+)%)/g;
  let total = 0;
  let canvas = 0;
  let read = 0;
  let match: RegExpExecArray | null;
  while ((match = term.exec(body)) !== null) {
    const sign = match[1] === "-" ? -1 : 1;
    const times = match[2] === undefined ? 1 : Number(match[2]);
    if (match[3] !== undefined) total += sign * times * lengthOf(match[3]);
    else canvas += (sign * times * Number(match[4])) / 100;
    read += 1;
  }
  // Un termine che il conto non sa leggere sparirebbe in silenzio e il totale
  // verrebbe più piccolo del vero, cioè nella direzione che rassicura. È già
  // successo qui: `--space-2` ha una cifra nel nome e il primo motivo lo
  // saltava, e la regola passava per un pareggio esatto invece che per otto
  // pixel di franco.
  const written = (body.match(/var\(|%/g) ?? []).length;
  expect(read, `il conto non sa leggere «${value}»`).toBe(written);

  // E CONTARE LE PAROLE NON BASTA: sono occorrenze, non termini. Un letterale
  // in `px` accanto a un `var()` passava di qui intatto — il numero dei `var(`
  // tornava — e spariva dal totale. Su `margin-left` l'errore allenta invece di
  // stringere: `calc(var(--controls-reserve) + 40px)` lasciava tutta la
  // batteria verde con la barra dentro la minimappa.
  //
  // Quindi il verso giusto non si chiede al conto ma al RESTO: consumati i
  // termini che sappiamo leggere, del corpo devono avanzare solo i separatori.
  // Qualunque altra cosa avanzi è un pezzo che nessuno ha misurato, e si
  // rifiuta invece di indovinare.
  const rest = body.replace(term, "").replace(/[+\-*\s]/g, "");
  expect(
    rest,
    `«${value}» porta «${rest}», che nessuno qui sa misurare: il totale lo salterebbe in silenzio`,
  ).toBe("");

  // E LA TELA SI CONTA, non si cancella. Il conto qui sotto non dipende dalla
  // larghezza della tela solo se il `100%` compare esattamente le volte che ci
  // si aspetta: una nel tetto, che si semplifica con il `100%` da cui parte la
  // fascia della minimappa, e nessuna nello scarto, che è una distanza fissa
  // dal fianco.
  expect(
    canvas,
    `«${value}» porta ${canvas} volte la larghezza della tela invece di ${canvasTimes}: ` +
      `il «100%» non si semplifica più, e il conto qui sotto varrebbe a una larghezza sola`,
  ).toBe(canvasTimes);
  return total;
}

/**
 * **LA BARRA DICHIARA IL CORRIDOIO CHE NON OCCUPA.**
 *
 * Due volte in questa stessa lavorazione la barra ha coperto la minimappa
 * passando in mezzo a tipi verdi e prove verdi: prima larga 540, poi 468. Tutte
 * e due le volte il rimedio è stato **spostare un numero**, e un numero non
 * diventa rosso. Centrare una barra larga quanto la somma dei suoi attrezzi
 * funziona a una larghezza sola: quella su cui è stata guardata.
 *
 * Qui non si misurano pixel — in jsdom niente ha dimensioni, e a misurare i
 * pixel questa prova sarebbe verde su un browser e muta su un altro. Si legge
 * la **regola**: da che punto la barra parte, e quanto del riquadro il suo
 * tetto le concede. Se il bordo destro peggiore che quei due numeri ammettono
 * finisce dentro la fascia della minimappa, la barra *può* coprirla — a
 * qualche larghezza, che è già abbastanza.
 *
 * E il conto non dipende dalla larghezza della tela, perché il `100%` compare
 * una volta da una parte e una dall'altra e si semplifica. È questo che lo fa
 * valere a **ogni** larghezza invece che a quella su cui qualcuno ha guardato,
 * ed è per questo che `sumOfTerms` lo **conta** invece di cancellarlo: era
 * l'unica cosa che qui si dava per buona senza interrogarla, e ci passavano due
 * aggiramenti da 200px con la batteria tutta verde.
 */
describe("il corridoio che la barra non occupa", () => {
  test("LA BARRA SI MISURA SUL CORRIDOIO, NON SULLA SOMMA DEI SUOI ATTREZZI", () => {
    const toolbar = declarationsOf(".toolbar");
    const controls = lengthOf("--controls-reserve");
    const minimap = lengthOf("--minimap-reserve");

    // Da dove parte il bordo sinistro, misurato dal fianco della tela: una
    // distanza fissa, quindi zero volte la larghezza della tela.
    const offset = toolbar.get("margin-left");
    expect(offset, "la barra non dichiara nessuno scarto dal fianco della tela").toBeDefined();
    const left = sumOfTerms(offset as string, 0);

    // Quanto il tetto le concede: la tela **una volta sola**, meno tutto ciò
    // che il `calc` toglie.
    const ceiling = toolbar.get("max-width");
    expect(ceiling, "la barra non dichiara nessun tetto di larghezza").toBeDefined();
    expect(
      (ceiling as string).replace(/\s+/g, " "),
      "il tetto della barra non parte dalla larghezza della tela",
    ).toMatch(/^calc\(100% -/);
    const kept = -sumOfTerms(ceiling as string, 1);

    // LA REGOLA. Bordo destro peggiore = 100% - kept + left. La minimappa
    // comincia a 100% - minimap. Il `100%` sparisce da tutte e due le parti — e
    // che ce ne sia esattamente uno per parte l'ha appena verificato
    // `sumOfTerms`, con il coefficiente 1 sul tetto e 0 sullo scarto.
    const reach = kept - left;
    const where =
      reach >= 0 ? `${reach}px dal fianco destro` : `${-reach}px OLTRE il fianco destro`;
    expect(
      left + minimap,
      `la barra può entrare nella fascia della minimappa: parte a ${left}px dal fianco sinistro ` +
        `e il suo tetto le lascia arrivare fino a ${where} della tela, mentre la minimappa ne occupa ${minimap}px`,
    ).toBeLessThanOrEqual(kept);

    // E dall'altro fianco, che i comandi di zoom restino scoperti.
    expect(
      left,
      `la barra parte a ${left}px e i comandi di zoom arrivano a ${controls}px`,
    ).toBeGreaterThanOrEqual(controls);
  });

  test("lo scarto conta solo se il pannello è ancorato a un fianco", () => {
    // `margin-left` su un pannello `bottom-center` non toglie la barra da
    // nessuna fascia: React Flow lo centra con una `translateX`, e lo scarto
    // dichiarato si limiterebbe a scentrarla. La regola qui sopra sarebbe vera
    // sul foglio e falsa a schermo.
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);
    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar.classList.contains("left")).toBe(true);
    expect(toolbar.classList.contains("center")).toBe(false);
  });

  test("LE RISERVE NON SONO NUMERI INVENTATI: le detta React Flow", () => {
    // Il conto qui sopra userebbe felicemente due riserve troppo piccole —
    // sono dichiarate nello stesso foglio che le verifica, e due copie che
    // sbagliano insieme si confermano. L'ancora sta fuori da tutte e due: gli
    // inquilini veri della fascia bassa, letti dove si leggono senza layout.
    const theirs = parseStylesheet(reactFlowSource);
    const declaration = (selector: string, property: string) => {
      const rule = theirs.rules.find((candidate) => candidate.selector === selector);
      expect(rule, `React Flow non ha più una regola «${selector}»`).toBeDefined();
      const value = new Map(rule!.declarations).get(property);
      expect(value, `«${selector}» non dichiara più «${property}»`).toBeDefined();
      return Number(/^(\d+(?:\.\d+)?)px$/.exec(value as string)?.[1]);
    };

    // Il margine con cui React Flow stacca OGNI pannello dal fianco: è quello
    // che tiene i comandi e la minimappa lontani dal bordo, ed è anche quello
    // che la barra si riscrive.
    const panelMargin = declaration(".react-flow__panel", "margin");
    const buttonWidth = declaration(".react-flow__controls-button", "width");

    // La minimappa non ha una larghezza nel foglio: la porta l'`svg` che
    // disegna, e un attributo esiste anche senza layout.
    const { container } = render(<App />);
    goToFlows();
    const minimap = container.querySelector(".react-flow__minimap svg") as SVGElement;
    expect(minimap, "la minimappa non è disegnata").not.toBeNull();
    const minimapWidth = Number(minimap.getAttribute("width"));
    expect(Number.isFinite(minimapWidth) && minimapWidth > 0).toBe(true);

    expect(
      lengthOf("--controls-reserve"),
      `i comandi di zoom occupano ${panelMargin + buttonWidth}px dal fianco`,
    ).toBeGreaterThanOrEqual(panelMargin + buttonWidth);
    expect(
      lengthOf("--minimap-reserve"),
      `la minimappa occupa ${panelMargin + minimapWidth}px dal fianco`,
    ).toBeGreaterThanOrEqual(panelMargin + minimapWidth);
  });

  test("gli attrezzi vanno a capo invece di sfondare il corridoio", () => {
    // Un tetto di larghezza su un contenitore flex non trattiene i figli: la
    // scatola si stringe al tetto e il contenuto esce lo stesso, sopra la
    // minimappa, mentre il `getBoundingClientRect` della barra racconta una
    // scatola stretta e ubbidiente. Senza queste righe la regola sarebbe vera
    // sulla carta e falsa a schermo.
    //
    // E SONO DUE, non una. Il `flex-wrap` della fila salva il caso in cui non
    // ci stanno tre gruppi affiancati; quello del gruppo salva il caso in cui
    // non ci sta un gruppo intero, e il corridoio scende sotto la larghezza di
    // un gruppo da tre attrezzi molto prima di sparire. Interrogarne uno solo
    // lasciava passare l'altro: tolto il `flex-wrap` di `.toolbar__group` la
    // batteria restava tutta verde mentre a 900px la barra copriva la
    // minimappa di 85px.
    for (const selector of [".toolbar__row", ".toolbar__group"]) {
      expect(
        declarationsOf(selector).get("flex-wrap"),
        `«${selector}» non manda a capo: un corridoio più stretto del suo contenuto lo fa uscire sopra la minimappa`,
      ).toBe("wrap");
    }
  });
});

describe("dove sta la barra", () => {
  test("STA DENTRO LA TELA, E NON SCORRE VIA CON ESSA", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const toolbar = container.querySelector(".toolbar") as HTMLElement;
    expect(toolbar).not.toBeNull();

    // Dentro la tela: non è più un pezzo della colonna accanto.
    expect(toolbar.closest(".react-flow")).not.toBeNull();
    expect(toolbar.closest(".rail")).toBeNull();

    // E non scorre via: `.react-flow__viewport` è l'elemento che porta la
    // `transform` di pan e zoom. Una barra dentro quello si allontanerebbe alla
    // prima trascinata, e lo screenshot a tela ferma non lo direbbe mai.
    expect(toolbar.closest(".react-flow__viewport")).toBeNull();
  });

  test("premere un attrezzo aggiunge davvero un passo al flusso a fuoco", () => {
    // Le prove qui sotto guardano la barra da sola, con un finto al posto di
    // `addStep`: da lì un attrezzo scollegato sembrerebbe funzionare. Questa
    // attraversa tutta la finestra e legge il conteggio che la colonna mostra.
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);

    const countOf = () => {
      const item = container.querySelector("button.rail__item") as HTMLElement;
      return item.querySelector(".rail__note")?.textContent ?? "";
    };
    const before = Number.parseInt(countOf(), 10);
    expect(Number.isFinite(before)).toBe(true);

    const check = container.querySelector(".toolbar__tool[data-kind='check']") as HTMLElement;
    fireEvent.click(check);

    expect(Number.parseInt(countOf(), 10)).toBe(before + 1);
  });

  test("la colonna non tiene più nessun attrezzo", () => {
    const { container } = render(<App />);
    goToFlows();
    focusAFlow(container);
    const rail = container.querySelector(".rail") as HTMLElement;
    expect(rail.querySelectorAll(".toolbar__tool")).toHaveLength(0);
    expect(rail.querySelectorAll(".palette__item")).toHaveLength(0);
  });
});

describe("cosa offre la barra", () => {
  test("OGNI FAMIGLIA CREA UN PASSO CHE IL MOTORE RICONOSCE", () => {
    const seen: StepKind[] = [];
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={(kind) => seen.push(kind)} onNewFlow={() => {}} />,
    );

    for (const tool of Array.from(container.querySelectorAll<HTMLElement>(".toolbar__tool"))) {
      fireEvent.click(tool);
    }

    expect(seen).toEqual(TOOLBAR_KINDS);
    for (const kind of seen) {
      const action = DEFAULT_ACTION_FOR_KIND[kind];
      // Non basta che l'azione esista: deve essere una di quelle che il
      // vocabolario del motore registra. È il guasto 41, riparato prima di
      // questo lavoro dentro `flow.ts` e mai interrogato da nessuno: sei nomi
      // inventati che creavano nodi che non si salvavano.
      expect(action, `la famiglia «${kind}» non ha un'azione`).toBeDefined();
      expect(KNOWN_ACTIONS, `l'azione «${String(action)}» non è nel vocabolario`).toContain(action);
    }
  });

  test("i gruppi coprono ESATTAMENTE le famiglie che hanno un'azione", () => {
    // Nei due versi: né un attrezzo senza azione (un bottone che non salva), né
    // una famiglia esistente lasciata fuori dalla cassetta senza che nessuno se
    // ne accorga. `wait` e `branch` non hanno azione e restano fuori da tutte e
    // due le liste.
    expect([...TOOLBAR_KINDS].sort()).toEqual([...KINDS_WITH_ACTION].sort());
    expect(TOOLBAR_KINDS).toHaveLength(7);
  });

  test("ogni attrezzo porta un segno E una parola", () => {
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={() => {}} onNewFlow={() => {}} />,
    );
    for (const tool of Array.from(container.querySelectorAll<HTMLElement>(".toolbar__tool"))) {
      // Il divieto 5 applicato alla forma: un segno da solo non porta niente,
      // come non lo porta il colore da solo.
      expect(tool.querySelector("svg.toolbar__mark")).not.toBeNull();
      expect(tool.querySelector(".toolbar__label")?.textContent?.trim()).toBeTruthy();
    }
  });

  test("ogni gruppo si nomina per chi legge con la voce", () => {
    const { container } = render(
      <Toolbar flowName="prima-corsa" onAdd={() => {}} onNewFlow={() => {}} />,
    );
    const groups = Array.from(container.querySelectorAll<HTMLElement>("[role='group']"));
    expect(groups).toHaveLength(TOOL_GROUPS.length);
    for (const group of groups) {
      expect(group.getAttribute("aria-label")).toBeTruthy();
    }
  });

  test("la barra dice in quale flusso finisce il passo", () => {
    const { container } = render(
      <Toolbar flowName="esamina-la-repo" onAdd={() => {}} onNewFlow={() => {}} />,
    );
    expect(container.querySelector(".toolbar__target")?.textContent).toContain("esamina-la-repo");
  });
});

describe("senza un flusso scelto", () => {
  test("LA BARRA DICE COSA MANCA, NON SI LIMITA A SPEGNERSI", () => {
    const onNewFlow = vi.fn();
    const { container } = render(<Toolbar flowName={null} onAdd={() => {}} onNewFlow={onNewFlow} />);

    // Nessun bottone spento: un pulsante che non si può premere e non dice
    // perché è un vicolo cieco.
    expect(container.querySelectorAll("button:disabled")).toHaveLength(0);
    expect(container.querySelectorAll(".toolbar__tool")).toHaveLength(0);

    // Al loro posto, ciò che manca — a schermo, non dentro un `title`.
    const prompt = container.querySelector(".toolbar__prompt") as HTMLElement;
    expect(prompt).not.toBeNull();
    expect(prompt.textContent).toContain("Scegli un flusso");

    // E il gesto che lo risolve, che funziona davvero.
    fireEvent.click(screen.getByRole("button", { name: /Nuovo flusso/ }));
    expect(onNewFlow).toHaveBeenCalledTimes(1);
  });

  test("il motivo non sta in un `title`, dove nessuno lo cerca", () => {
    const { container } = render(<Toolbar flowName={null} onAdd={() => {}} onNewFlow={() => {}} />);
    const withTitle = Array.from(container.querySelectorAll("[title]"));
    expect(withTitle).toEqual([]);
  });
});

/**
 * **CON ZERO FLUSSI LA BARRA NON C'È AFFATTO**, e non è la stessa cosa di
 * «nessun flusso a fuoco»: lì la barra resta e cambia mestiere. Quel momento è
 * della tela vuota, che insegna il primo gesto; due inviti nello stesso schermo
 * si annullano.
 *
 * La condizione sta in una riga sola di `App.tsx` (`flows.size > 0`), e finora
 * nessun file la interrogava: toglierla lasciava tutta la batteria verde e
 * metteva due «+ Nuovo flusso» nello stesso schermo.
 */
describe("con zero flussi", () => {
  test("LA BARRA SPARISCE E RESTA LA TELA VUOTA, non tutte e due", () => {
    sample.empty = true;
    const { container } = render(<App />);
    goToFlows();

    expect(container.querySelector(".blank"), "la tela vuota non c'è").not.toBeNull();
    expect(container.querySelector(".toolbar"), "la barra invita insieme alla tela vuota").toBeNull();
  });

  test("con un flusso vale il contrario: la barra c'è e la tela vuota no", () => {
    // Nei due versi, se no «nessuna barra» sarebbe vero anche per una barra
    // che non compare mai.
    const { container } = render(<App />);
    goToFlows();
    expect(container.querySelector(".toolbar")).not.toBeNull();
    expect(container.querySelector(".blank")).toBeNull();
  });
});
