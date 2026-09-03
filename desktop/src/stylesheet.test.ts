import { describe, expect, test } from "vitest";
import stylesheetSource from "./styles.css?raw";
import { parseColor, parseStylesheet, resolveVars } from "./contrast";

/**
 * **THE PROHIBITIONS AT THE TOP OF `styles.css`, INTERROGATED.** A prohibition
 * without a check does not hold even for the person writing it, in the turn
 * they write it. Whatever can be read off the sheet is read here; rule 6, the
 * contrast pairs, is measured on the painted DOM in `contrast.test.tsx`.
 */

const sheet = parseStylesheet(stylesheetSource);

/** The sheet's rules minus `:root`, which is where the roles are defined. */
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
    // A `system-ui` stack on a tool's monogram, and two monospace stacks copied
    // into the console and the code boxes.
    const wrong = declarationsOf("font-family").filter(
      ({ value }) => !/^var\(--font-(display|prose|data)\)$/.test(value),
    );
    expect(wrong).toEqual([]);
  });
});

describe("divieto 2 — due raggi e una pillola", () => {
  test("nessun raggio scritto a mano", () => {
    // `border-radius: 2px` on a lane's mark, `50%` on a trigger's. The rule
    // admits three values, and they are three roles.
    const allowed = /^var\(--radius(-lg|-pill)?\)$/;
    const wrong = declarationsOf("border-radius").filter(({ value }) => !allowed.test(value));
    expect(wrong).toEqual([]);
  });
});

describe("divieto 3 — una sola ombra", () => {
  test("l'unica ombra è `--shadow`; un anello interno non è un'ombra", () => {
    // `inset` floats nothing: it is the focus hairline drawn inside the border,
    // and the rule is about what sits above the canvas.
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
    // No literal colour outside `:root`. It is the precondition for a dark
    // theme: with every tint coming from a role, the second theme is blocked
    // only by the measurements it still needs.
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
    // A misspelled role does not fail: it resolves to nothing and the element
    // inherits its container's colour. Nobody notices.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    expect(root).toBeDefined();
    const tokens = (root as { declarations: Array<[string, string]> }).declarations.filter(
      ([property]) => /^--(bg|paper|raised|rail|line|band-fill|ink|muted|faint|state-|ok|warn|danger|focus|ink-surface|on-ink|optional|lane-)/.test(property),
    );
    expect(tokens.length).toBeGreaterThan(15);
    // A role may stand for another: the names a component asks for are
    // aliases. Resolved before reading, so a misspelt one is still red.
    const roles = new Map((root as { declarations: Array<[string, string]> }).declarations);
    const broken = tokens.filter(([, value]) => parseColor(resolveVars(value, roles)) === null);
    expect(broken).toEqual([]);
  });
});

describe("il foglio si legge tutto", () => {
  test("QUELLO CHE ARRIVA QUI È IL FOGLIO SCRITTO, non una copia lavorata", () => {
    // `vitest` returns an empty string for every CSS import until told
    // `css: true`: without this line every check in this file would be green
    // for having read nothing.
    expect(stylesheetSource).toContain("WHAT THIS DIRECTION FORBIDS");
    expect(stylesheetSource.length).toBeGreaterThan(40000);
    expect(sheet.rules.length).toBeGreaterThan(200);
  });

  test("nessun colore dentro una regola-@", () => {
    expect(sheet.colorsInsideAtRules).toBe(0);
  });

  test("THE DARK SCHEME REDEFINES EVERY ROLE THAT CARRIES A COLOUR, and no other", () => {
    // A role left out keeps its light value under the dark ground: legible in
    // the test that never looked, unreadable on the screen. So the two sets of
    // colour roles must be the same set, and every dark value must be a colour.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    // Raw, not resolved: an alias follows its target under the dark ground.
    const lightColours = (root?.declarations ?? []).filter(([, value]) => parseColor(value) !== null).map(([name]) => name);
    const dark = sheet.darkRoot ?? [];
    const darkNames = dark.map(([name]) => name);
    expect(lightColours.length).toBeGreaterThan(30);
    expect(lightColours.filter((name) => !darkNames.includes(name))).toEqual([]);
    expect(darkNames.filter((name) => !lightColours.includes(name) && !/^--(shadow|scrim)$/.test(name))).toEqual([]);
    expect(dark.filter(([name, value]) => parseColor(value) === null && !/^--(shadow|scrim)$/.test(name))).toEqual([]);
  });

  test("il divieto 7 è scritto nel foglio, non solo nel commento", () => {
    // `--faint` was abolished by making it identical to `--muted`. If somebody
    // lightens it again, this line says so before the contrast check does.
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

/**
 * ZOOMING IN MUST NOT SHOW LESS. The far node wrapped the name and read it
 * whole; the near node clipped it — so approaching a node lost information,
 * which is the opposite of what the gesture asks. It wraps at every zoom now.
 */
describe("the name of a step reads the same at every zoom", () => {
  function wrapping(selector: string): Map<string, string> {
    const found = new Map<string, string>();
    for (const rule of sheet.rules) {
      if (rule.selector.trim() !== selector) continue;
      for (const [property, value] of rule.declarations) {
        if (["white-space", "overflow-wrap", "-webkit-line-clamp"].includes(property)) {
          found.set(property, value.trim());
        }
      }
    }
    return found;
  }

  test("the far node declares no wrapping of its own", () => {
    const far = wrapping(".step-node[data-far] .step-node__id");
    expect([...far.keys()], "the two zooms disagree about how the name wraps").toEqual([]);
  });

  /* Three and not two: measured in a browser, two lines clip the longest step
     name that exists, and four stops discriminating — a control string of 80
     characters comes back unclipped, which means the measure has gone blind. */
  test("the name wraps, and stops at three lines", () => {
    const near = wrapping(".step-node__id");
    expect(near.get("white-space")).toBe("normal");
    expect(near.get("-webkit-line-clamp")).toBe("3");
  });
});
