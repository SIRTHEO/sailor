import { describe, expect, test } from "vitest";
import stylesheetSource from "./styles.css?raw";
import { parseColor, parseStylesheet } from "./contrast";

/**
 * **I DIVIETI DICHIARATI IN TESTA A `styles.css`, INTERROGATI.**
 *
 * Il cartiglio in cima al foglio elenca undici divieti numerati. Nel turno
 * stesso in cui sono stati scritti ne sono stati violati tre — un raggio a
 * mano, una seconda ombra, una quarta famiglia di caratteri — e altri tre
 * stavano lì da prima. Un divieto senza un controllo non tiene nemmeno per chi
 * lo scrive, nel turno in cui lo scrive: è appena successo.
 *
 * Quello che si può leggere dal foglio si legge qui. Il divieto 6 — le
 * accoppiate di contrasto — non si legge da un foglio: va misurato sul DOM
 * disegnato, e sta in `contrast.test.tsx`.
 */

const sheet = parseStylesheet(stylesheetSource);

/** Le regole del foglio meno il blocco `:root`, che è dove i ruoli si definiscono. */
const outsideRoot = sheet.rules.filter((rule) => rule.selector !== ":root");

function declarationsOf(property: string): Array<{ selector: string; value: string }> {
  const found: Array<{ selector: string; value: string }> = [];
  for (const rule of outsideRoot) {
    for (const [name, value] of rule.declarations) {
      if (name === property) found.push({ selector: rule.selector, value: value.trim() });
    }
  }
  return found;
}

describe("divieto 1 — tre famiglie di caratteri, e solo quelle", () => {
  test("nessuna pila di caratteri scritta a mano fuori dai ruoli", () => {
    // `ui-sans-serif, system-ui, sans-serif` sul monogramma di uno strumento,
    // e due pile monospaziate copiate nella console e nei riquadri di codice.
    const wrong = declarationsOf("font-family").filter(
      ({ value }) => !/^var\(--font-(display|prose|data)\)$/.test(value),
    );
    expect(wrong).toEqual([]);
  });
});

describe("divieto 2 — due raggi e una pillola", () => {
  test("nessun raggio scritto a mano", () => {
    // `border-radius: 2px` sul segno di una corsia, `50%` sul segno di un
    // innesco. Il divieto ammette tre valori, e sono tre ruoli.
    const allowed = /^var\(--radius(-lg|-pill)?\)$/;
    const wrong = declarationsOf("border-radius").filter(({ value }) => !allowed.test(value));
    expect(wrong).toEqual([]);
  });
});

describe("divieto 3 — una sola ombra", () => {
  test("l'unica ombra è `--shadow`; un anello interno non è un'ombra", () => {
    // `inset` non fa galleggiare niente: è il filo del fuoco disegnato dentro
    // il bordo, e il divieto parla di ciò che sta sopra la tela.
    const wrong = declarationsOf("box-shadow").filter(
      ({ value }) => value !== "var(--shadow)" && value !== "none" && !value.startsWith("inset "),
    );
    expect(wrong).toEqual([]);
  });
});

describe("divieto 8 — i corpi stanno nella scala", () => {
  test("nessun corpo scritto a mano", () => {
    const wrong = declarationsOf("font-size").filter(
      ({ value }) => !/^var\(--text-[a-z]+\)$/.test(value),
    );
    expect(wrong).toEqual([]);
  });
});

describe("ogni colore passa da un ruolo", () => {
  test("FUORI DA `:root` NON C'È NESSUN COLORE LETTERALE", () => {
    // È la condizione che il cartiglio dichiarava mancante — «restano ~40 tinte
    // scritte a mano» — mentre era già quasi vera: le tinte residue erano due,
    // e chi leggeva credeva che restasse un lavoro che non c'era. Adesso è
    // vera del tutto, e il tema scuro resta bloccato solo dalle misure che gli
    // mancano, non da un debito immaginario.
    const literal =
      /#[0-9a-fA-F]{3,8}\b|\brgba?\([^)]*\)|\bhsla?\([^)]*\)|\b(?:white|black|red|blue|green|gray|grey|orange|purple)\b/;
    const wrong: Array<{ selector: string; property: string; value: string }> = [];
    for (const rule of outsideRoot) {
      for (const [property, value] of rule.declarations) {
        if (property === "white-space") continue;
        if (literal.test(value)) wrong.push({ selector: rule.selector, property, value });
      }
    }
    expect(wrong).toEqual([]);
  });

  test("ogni ruolo di `:root` che nomina un colore è un colore leggibile", () => {
    // Un ruolo scritto male non fallisce: si risolve in niente, e l'elemento
    // eredita il colore di chi lo contiene. Nessuno se ne accorge.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    expect(root).toBeDefined();
    const tokens = (root as { declarations: Array<[string, string]> }).declarations.filter(
      ([property]) => /^--(bg|paper|raised|rail|line|band-fill|ink|muted|faint|state-|ok|warn|danger|focus|ink-surface|on-ink|optional|lane-)/.test(property),
    );
    expect(tokens.length).toBeGreaterThan(15);
    const broken = tokens.filter(([, value]) => parseColor(value) === null);
    expect(broken).toEqual([]);
  });
});

describe("il foglio si legge tutto", () => {
  test("QUELLO CHE ARRIVA QUI È IL FOGLIO SCRITTO, non una copia lavorata", () => {
    // `vitest` restituisce una stringa vuota per ogni import di CSS finché non
    // gli si dice `css: true`: senza questa riga tutti i controlli di questo
    // file sarebbero verdi per non aver letto niente.
    expect(stylesheetSource).toContain("COSA QUESTA DIREZIONE VIETA");
    expect(stylesheetSource.length).toBeGreaterThan(40000);
    expect(sheet.rules.length).toBeGreaterThan(200);
  });

  test("nessun colore dentro una regola-@", () => {
    expect(sheet.colorsInsideAtRules).toBe(0);
  });

  test("il divieto 7 è scritto nel foglio, non solo nel commento", () => {
    // `--faint` è stato abolito rendendolo identico a `--muted`. Se qualcuno lo
    // schiarisce di nuovo, questa riga lo dice prima del contrasto.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    const declarations = new Map((root as { declarations: Array<[string, string]> }).declarations);
    expect(declarations.get("--faint")).toBe(declarations.get("--muted"));
  });
});

/**
 * **DIVIETO 11 — NESSUNA COLONNA FISSA SENZA UNA VIA D'USCITA.**
 *
 * Un elemento con una `width` in pixel e `flex-shrink: 0` tiene quella
 * larghezza qualunque sia la finestra: è il suo scopo. Ma due di quegli
 * elementi ai lati di una tela flessibile si spartiscono la finestra prima
 * che la tela abbia voce, e sotto una certa larghezza **la tela va a zero**.
 *
 * Misurato il 01/09/2026 con un browser vero, appena aperta la vista dei
 * flussi:
 *
 *     375px →  colonna 232 · TELA 0 · pannello 288   (232+288 = 520 > 375)
 *    1440px →  colonna 232 · tela 920 · pannello 288
 *
 * A 375 pixel la superficie principale del prodotto non è stretta, non è
 * coperta, non è sotto: **non c'è**, e nessuna delle 12.494 righe di questo
 * albero diventava rossa per dirlo. In 2750 righe di foglio non esisteva una
 * sola regola-@ di impaginazione — le due che c'erano parlano entrambe di
 * movimento.
 *
 * La misura completa, che una prova sul foglio non può fare, sta in
 * `npm run check:canvas`: quella guarda la geometria disegnata. Questa guarda
 * la causa, ed è la più economica delle due — gira in millisecondi, dentro la
 * batteria, senza browser.
 */
describe("divieto 11 — una colonna fissa dichiara come si comporta da stretta", () => {
  /** La finestra più stretta che questo progetto dichiara di sostenere: è la
   *  larghezza a cui `scripts/screenshots.ts` cattura, cioè quella a cui
   *  qualcuno ha già deciso che la finestra va guardata. */
  const NARROWEST = 375;

  /**
   * Le COLONNE rigide: larghezza fissa in pixel, `flex-shrink: 0`, **e uno
   * scorrimento proprio**.
   *
   * L'ultima condizione non è un dettaglio, è ciò che separa una colonna da un
   * pallino. Alla prima scrittura questa prova pescava anche `.focusbar__dot`
   * (9px), `.flow-band__mark` (10px) e `.trigger-node__mark` (7px): segni
   * grafici che sono rigidi di proposito e non impaginano niente. Un controllo
   * che li avesse contati avrebbe fatto scrivere regole-@ sui puntini — cioè
   * avrebbe fatto cambiare il mondo per un numero sbagliato.
   *
   * Un elemento che scorre da sé **contiene** qualcosa: è una colonna. È un
   * criterio strutturale, non una soglia di pixel scelta a occhio.
   */
  const rigid = outsideRoot
    .map((rule) => {
      const declarations = new Map(rule.declarations);
      const width = declarations.get("width")?.trim();
      const shrink = declarations.get("flex-shrink")?.trim();
      const scrolls = declarations.get("overflow-y")?.trim();
      const pixels = width?.match(/^(\d+)px$/);
      if (!pixels || shrink !== "0") return null;
      if (scrolls !== "auto" && scrolls !== "scroll") return null;
      return { selector: rule.selector, width: Number(pixels[1]) };
    })
    .filter((found): found is { selector: string; width: number } => found !== null);

  test("la prova guarda le colonne, non i pallini", () => {
    // Se un giorno questo elenco si svuota, la prova sotto diventa verde per
    // non aver guardato niente — ed è il modo in cui un controllo muore in
    // silenzio.
    expect(rigid.length).toBeGreaterThan(0);
    expect(rigid.every((column) => column.width >= 100)).toBe(true);
  });

  test("le colonne rigide non si mangiano da sole la finestra più stretta", () => {
    const total = rigid.reduce((sum, column) => sum + column.width, 0);
    if (total < NARROWEST) return; // ci stanno: non serve nessuna via d'uscita

    // Non ci stanno. Allora ognuna deve comparire dentro una regola-@ che ne
    // cambia la larghezza o la toglie di mezzo: senza, ciò che sta in mezzo
    // viene schiacciato a zero e nessuno lo vede.
    const atRules = stylesheetSource.match(/@media[^{]+\{(?:[^{}]|\{[^{}]*\})*\}/g) ?? [];
    const inside = atRules.join("\n");
    const unguarded = rigid.filter(({ selector }) => !inside.includes(selector));

    expect({
      totale: `${total}px di colonne rigide contro una finestra di ${NARROWEST}px`,
      senzaViaDUscita: unguarded.map((column) => `${column.selector} (${column.width}px)`),
    }).toEqual({
      totale: `${total}px di colonne rigide contro una finestra di ${NARROWEST}px`,
      senzaViaDUscita: [],
    });
  });
});

/**
 * PROHIBITION 9, which had nothing asking after it. Writing the corner marks I
 * put a gradient in this sheet — two identical stops, a solid rectangle spelt
 * as a gradient — and the whole battery stayed green. Put that line back and
 * this goes red, naming the rule it is in.
 */
describe("prohibition 9 — no gradients, frosted glass or blur", () => {
  test("no rule in the sheet declares one", () => {
    const forbidden = /(linear|radial|conic)-gradient|backdrop-filter|blur\s*\(/i;
    const guilty = sheet.rules
      .flatMap((rule) => rule.declarations.map(([property, value]) => ({ rule, property, value })))
      .filter(({ value }) => forbidden.test(value))
      .map(({ rule, property, value }) => `${rule.selector} { ${property}: ${value} }`);
    expect(guilty, "a gradient got into the sheet").toEqual([]);
  });
});
