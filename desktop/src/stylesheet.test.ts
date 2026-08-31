import { describe, expect, test } from "vitest";
import stylesheetSource from "./styles.css?raw";
import { parseColor, parseStylesheet } from "./contrast";

/**
 * **I DIVIETI DICHIARATI IN TESTA A `styles.css`, INTERROGATI.**
 *
 * Il cartiglio in cima al foglio elenca dieci divieti numerati. Nel turno
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
      ([property]) => /^--(bg|paper|raised|rail|line|band-fill|ink|muted|faint|state-|ok|warn|danger|focus)/.test(property),
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
