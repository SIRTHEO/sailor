/**
 * **IL DIVIETO 6 MISURATO DA UN CONTROLLO, NON DA UN ATTREZZO USA-E-GETTA.**
 *
 * Il divieto in testa a `styles.css` dice: nessuna accoppiata testo/sfondo
 * sotto 4,5:1. Fino al 31/08/2026 quel divieto non aveva niente che lo
 * interrogasse: la misura era stata presa una volta a mano, in un turno, con
 * uno script morto insieme al turno. La prova che questo file rende possibile
 * nasce rossa se qualcuno rimette `--muted: #94a3b8` — due caratteri — mentre
 * `vitest`, `tsc` e `identifiers_are_in_english` restano tutti e tre verdi.
 *
 * **PERCHÉ UN MOTORE NOSTRO E NON UN BROWSER.** Servirebbe un browser vero, e
 * un browser vero qui vuol dire una dipendenza in più (`playwright` non c'è) e
 * una porta in ascolto — che dentro il perimetro dei comandi risponde `EPERM`.
 * Un controllo che si salta da solo quando non può girare è esattamente il buco
 * che stiamo chiudendo. Quindi: `jsdom` disegna il DOM vero dei componenti
 * veri, e qui sotto c'è la parte che a `jsdom` manca — la cascata, l'eredità
 * delle proprietà personalizzate e `var()`.
 *
 * **COME SI SA CHE QUESTO MOTORE NON MENTE.** Non basta che sia d'accordo con
 * se stesso: due copie che sbagliano insieme si confermano. Le sue misure sono
 * state confrontate con quelle di Chrome headless sulla stessa pagina — stesso
 * elenco di accoppiate, stessi rapporti a meno del centesimo — e con quelle di
 * un verificatore che questo file non l'aveva scritto.
 *
 * **COSA QUESTO MOTORE NON FA**, e va saputo prima di fidarsene: non fa
 * impaginazione (niente sovrapposizioni, niente altezze), non valuta gli stati
 * del puntatore (`:hover`, `:focus`) né gli pseudo-elementi, e salta il
 * contenuto delle regole-@. Le regole saltate che dichiarano un colore vengono
 * contate: se qualcuno ne scrive una, il controllo lo dice invece di tacere.
 */

export interface Rgb {
  r: number;
  g: number;
  b: number;
  /** 0 = del tutto trasparente, 1 = pieno. */
  a: number;
}

const TRANSPARENT: Rgb = { r: 0, g: 0, b: 0, a: 0 };

/** Il fondo ultimo sotto la pagina: quello che il browser dipinge da sé. */
export const CANVAS_WHITE: Rgb = { r: 255, g: 255, b: 255, a: 1 };

function channel(text: string, scale: number): number {
  const clean = text.trim();
  if (clean.endsWith("%")) return (Number.parseFloat(clean) / 100) * scale;
  return Number.parseFloat(clean);
}

/**
 * Le forme che questo foglio usa davvero: `transparent`, esadecimale a 3, 4, 6
 * o 8 cifre, e `rgb()`/`rgba()` con le virgole o con gli spazi e la barra.
 * Tutto il resto torna `null` — meglio non sapere che indovinare.
 */
export function parseColor(text: string): Rgb | null {
  const value = text.trim();
  if (value === "" || value === "none") return null;
  if (value === "transparent") return { ...TRANSPARENT };

  const hex = /^#([0-9a-f]{3,8})$/i.exec(value);
  if (hex) {
    const digits = hex[1];
    const wide = digits.length > 4;
    const step = wide ? 2 : 1;
    if (digits.length !== 3 && digits.length !== 4 && digits.length !== 6 && digits.length !== 8) {
      return null;
    }
    const at = (index: number) => {
      const piece = digits.slice(index * step, index * step + step);
      return Number.parseInt(wide ? piece : piece + piece, 16);
    };
    const hasAlpha = digits.length === 4 || digits.length === 8;
    return { r: at(0), g: at(1), b: at(2), a: hasAlpha ? at(3) / 255 : 1 };
  }

  const call = /^rgba?\(([^)]*)\)$/i.exec(value);
  if (call) {
    const [head, tail] = call[1].split("/");
    const parts = head.trim().split(/[\s,]+/).filter((piece) => piece !== "");
    if (parts.length < 3) return null;
    const alphaText = tail ?? parts[3];
    return {
      r: channel(parts[0], 255),
      g: channel(parts[1], 255),
      b: channel(parts[2], 255),
      a: alphaText === undefined ? 1 : channel(alphaText, 1),
    };
  }

  return null;
}

/** Sovrappone `front` a `back`, che si assume opaco. */
export function composite(front: Rgb, back: Rgb): Rgb {
  const a = Math.min(1, Math.max(0, front.a));
  return {
    r: front.r * a + back.r * (1 - a),
    g: front.g * a + back.g * (1 - a),
    b: front.b * a + back.b * (1 - a),
    a: 1,
  };
}

function luminance(color: Rgb): number {
  const straighten = (raw: number) => {
    const v = raw / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  return (
    0.2126 * straighten(color.r) + 0.7152 * straighten(color.g) + 0.0722 * straighten(color.b)
  );
}

/** Il rapporto di contrasto WCAG fra due colori opachi. */
export function contrastRatio(first: Rgb, second: Rgb): number {
  const a = luminance(first);
  const b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// ── il foglio di stile, letto come regole ──────────────────────────────

export interface CssRule {
  selector: string;
  specificity: number;
  order: number;
  declarations: Array<[string, string]>;
}

export interface Stylesheet {
  rules: CssRule[];
  /**
   * Quante dichiarazioni di colore stanno dentro una regola-@ saltata. Deve
   * restare zero: se qualcuno ci mette un colore, questo motore non lo vede e
   * il controllo diventerebbe cieco in silenzio.
   */
  colorsInsideAtRules: number;
  /**
   * The roles the OTHER scheme redefines, read from the one at-rule this
   * engine looks into: `@media (prefers-color-scheme: …) { :root … }`. The
   * sheet's ground is night, so the other one is light. `null` when there is
   * no second scheme.
   */
  otherRoot: Array<[string, string]> | null;
}

/** Whether an at-rule prelude is a colour scheme's, in any spacing. */
function isOtherScheme(prelude: string): boolean {
  return /^@media\s*\(\s*prefers-color-scheme\s*:\s*(dark|light)\s*\)$/.test(prelude);
}

/**
 * The same sheet under the other scheme, so every measurement made once can be
 * made again. A role the other scheme leaves out keeps the first one's value —
 * exactly what the browser does, and what the stylesheet test refuses.
 */
export function inOtherScheme(sheet: Stylesheet): Stylesheet {
  if (sheet.otherRoot === null) return sheet;
  const dark = new Map(sheet.otherRoot);
  return {
    ...sheet,
    rules: sheet.rules.map((rule) =>
      rule.selector === ":root"
        ? {
            ...rule,
            declarations: rule.declarations.map(
              ([property, value]) => [property, dark.get(property) ?? value] as [string, string],
            ),
          }
        : rule,
    ),
  };
}

/** Toglie i commenti senza toccare il resto. */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Uno stato che questo motore non sa disegnare: la regola si salta. */
const STATEFUL = /::|:hover|:focus|:active|:disabled|:checked|:target|:visited|:enabled/i;

/** Vero se questa dichiarazione porta un colore, comunque si chiami. */
function carriesColor(property: string, value: string): boolean {
  if (parseColor(value.trim()) !== null) return true;
  return /(^|-)(color|background|fill|stroke)$/.test(property);
}

function specificityOf(selector: string): number {
  const ids = selector.match(/#[\w-]+/g)?.length ?? 0;
  const classes = selector.match(/\.[\w-]+|\[[^\]]*\]|:[\w-]+(\([^)]*\))?/g)?.length ?? 0;
  const types = selector.match(/(^|[\s>+~])[a-z][\w-]*/gi)?.length ?? 0;
  return ids * 10000 + classes * 100 + types;
}

/** Spezza su un separatore solo fuori da parentesi, virgolette e parentesi quadre. */
function splitTop(text: string, separator: string): string[] {
  const pieces: string[] = [];
  let depth = 0;
  let quote = "";
  let current = "";
  for (const character of text) {
    if (quote !== "") {
      current += character;
      if (character === quote) quote = "";
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      current += character;
      continue;
    }
    if (character === "(" || character === "[") depth += 1;
    if (character === ")" || character === "]") depth -= 1;
    if (character === separator && depth === 0) {
      pieces.push(current);
      current = "";
      continue;
    }
    current += character;
  }
  pieces.push(current);
  return pieces;
}

function parseDeclarations(body: string): Array<[string, string]> {
  const declarations: Array<[string, string]> = [];
  for (const piece of splitTop(body, ";")) {
    const colon = piece.indexOf(":");
    if (colon < 0) continue;
    const property = piece.slice(0, colon).trim().toLowerCase();
    const value = piece.slice(colon + 1).trim();
    if (property === "" || value === "") continue;
    declarations.push([property, value]);
  }
  return declarations;
}

export function parseStylesheet(source: string): Stylesheet {
  const text = stripComments(source);
  const rules: CssRule[] = [];
  let colorsInsideAtRules = 0;
  let otherRoot: Array<[string, string]> | null = null;
  let order = 0;
  let index = 0;

  while (index < text.length) {
    const open = text.indexOf("{", index);
    if (open < 0) break;
    let depth = 1;
    let close = open + 1;
    while (close < text.length && depth > 0) {
      if (text[close] === "{") depth += 1;
      if (text[close] === "}") depth -= 1;
      close += 1;
    }
    // A STATEMENT AT-RULE IS NOT A PRELUDE: `@import` ends at its semicolon.
    // Read together with the selector that follows, `:root` itself looked like
    // the body of an at-rule and every role went unseen.
    const prelude = (splitTop(text.slice(index, open), ";").pop() ?? "").trim();
    const body = text.slice(open + 1, close - 1);
    index = close;

    if (isOtherScheme(prelude)) {
      // The other scheme is read as a sheet of its own: its `:root` is the
      // second set of roles, and any colour it writes elsewhere is as blind
      // to the measurement as a colour in any other at-rule.
      const inner = parseStylesheet(body);
      for (const rule of inner.rules) {
        if (rule.selector === ":root") {
          otherRoot = [...(otherRoot ?? []), ...rule.declarations];
          continue;
        }
        for (const [property, value] of rule.declarations) {
          if (carriesColor(property, value)) colorsInsideAtRules += 1;
        }
      }
      colorsInsideAtRules += inner.colorsInsideAtRules;
      continue;
    }

    // `@theme` DECLARES ROLES, IT PAINTS NOTHING. Tailwind emits it as custom
    // properties on `:root`, so that is what it is read as.
    if (/^@theme\b/.test(prelude)) {
      const declarations = parseDeclarations(body);
      if (declarations.length > 0) {
        order += 1;
        rules.push({ selector: ":root", specificity: specificityOf(":root"), order, declarations });
      }
      continue;
    }

    if (prelude.startsWith("@")) {
      for (const [property, value] of parseDeclarations(body)) {
        if (carriesColor(property, value)) colorsInsideAtRules += 1;
      }
      continue;
    }

    const declarations = parseDeclarations(body);
    if (declarations.length === 0) continue;
    order += 1;
    for (const selector of splitTop(prelude, ",")) {
      const clean = selector.trim();
      if (clean === "" || STATEFUL.test(clean)) continue;
      rules.push({ selector: clean, specificity: specificityOf(clean), order, declarations });
    }
  }

  return { rules, colorsInsideAtRules, otherRoot };
}

// ── la cascata, l'eredità e `var()` ────────────────────────────────────

/** Risolve `var(--nome, ripiego)` fino in fondo, coi ripieghi annidati. */
export function resolveVars(value: string, vars: Map<string, string>, depth = 0): string {
  if (depth > 12 || !value.includes("var(")) return value;
  const start = value.indexOf("var(");
  let cursor = start + 4;
  let level = 1;
  while (cursor < value.length && level > 0) {
    if (value[cursor] === "(") level += 1;
    if (value[cursor] === ")") level -= 1;
    cursor += 1;
  }
  const inside = value.slice(start + 4, cursor - 1);
  const comma = splitTop(inside, ",");
  const name = comma[0].trim();
  const fallback = comma.slice(1).join(",").trim();
  const known = vars.get(name);
  const replacement = known !== undefined ? known : fallback;
  const next = value.slice(0, start) + replacement + value.slice(cursor);
  return resolveVars(next, vars, depth + 1);
}

export interface ElementStyle {
  vars: Map<string, string>;
  /**
   * Le dichiarazioni che vincono su questo elemento, coi `var()` già sciolti.
   * Non è solo colore: è da qui che `layout.test.tsx` legge i corpi e le
   * interlinee dell'intestazione di una corsia, invece di ricopiarli.
   */
  declarations: Map<string, string>;
  color: Rgb;
  /** Il fondo effettivo sotto il testo di questo elemento, già opaco. */
  backdrop: Rgb;
  /** L'opacità accumulata dalla radice fin qui. */
  opacity: number;
  hidden: boolean;
}

/**
 * Le proprietà che contano qui. `color` si eredita, gli altri no — ed è
 * l'unica eredità che serve, perché lo sfondo si ricostruisce risalendo.
 */
function declarationsFor(element: Element, sheet: Stylesheet): Map<string, string> {
  const matched: Array<{ rule: CssRule; property: string; value: string }> = [];
  for (const rule of sheet.rules) {
    let hit = false;
    try {
      hit = element.matches(rule.selector);
    } catch {
      hit = false;
    }
    if (!hit) continue;
    for (const [property, value] of rule.declarations) matched.push({ rule, property, value });
  }
  matched.sort((left, right) => {
    if (left.rule.specificity !== right.rule.specificity) {
      return left.rule.specificity - right.rule.specificity;
    }
    return left.rule.order - right.rule.order;
  });

  const declarations = new Map<string, string>();
  for (const { property, value } of matched) declarations.set(property, value);
  // Lo stile in linea vince su tutto: è così che arrivano il colore di una
  // corsia e la tinta del segno di uno strumento.
  const inline = element.getAttribute("style");
  if (inline !== null) {
    for (const [property, value] of parseDeclarations(inline)) declarations.set(property, value);
  }
  return declarations;
}

function backgroundOf(declarations: Map<string, string>, vars: Map<string, string>): Rgb | null {
  const shorthand = declarations.get("background");
  const longhand = declarations.get("background-color");
  const raw = longhand ?? shorthand;
  if (raw === undefined) return null;
  return parseColor(resolveVars(raw, vars));
}

/**
 * Cammina il DOM disegnato e calcola, per ogni elemento, i colori che il
 * browser gli darebbe. Il fondo si compone scendendo: ogni elemento posa il
 * proprio sfondo — con la sua alfa moltiplicata per l'opacità accumulata —
 * sopra quello di chi lo contiene.
 */
export function styleTree(root: Element, sheet: Stylesheet): Map<Element, ElementStyle> {
  const styles = new Map<Element, ElementStyle>();

  const visit = (element: Element, inherited: ElementStyle) => {
    const declarations = declarationsFor(element, sheet);

    const vars = new Map(inherited.vars);
    for (const [property, value] of declarations) {
      if (property.startsWith("--")) vars.set(property, value);
    }

    const own = declarations.get("color");
    const resolvedColor = own === undefined ? null : parseColor(resolveVars(own, vars));
    const color = resolvedColor ?? inherited.color;

    const declaredOpacity = declarations.get("opacity");
    const factor = declaredOpacity === undefined ? 1 : Number.parseFloat(resolveVars(declaredOpacity, vars));
    const opacity = inherited.opacity * (Number.isFinite(factor) ? factor : 1);

    const background = backgroundOf(declarations, vars);
    const backdrop =
      background === null || background.a === 0
        ? inherited.backdrop
        : composite({ ...background, a: background.a * opacity }, inherited.backdrop);

    const hidden =
      inherited.hidden ||
      declarations.get("display") === "none" ||
      declarations.get("visibility") === "hidden";

    const resolved = new Map<string, string>();
    for (const [property, value] of declarations) {
      resolved.set(property, property.startsWith("--") ? value : resolveVars(value, vars));
    }

    const style: ElementStyle = { vars, declarations: resolved, color, backdrop, opacity, hidden };
    styles.set(element, style);
    for (const child of Array.from(element.children)) visit(child, style);
  };

  visit(root, {
    vars: new Map(),
    declarations: new Map(),
    color: { r: 0, g: 0, b: 0, a: 1 },
    backdrop: CANVAS_WHITE,
    opacity: 1,
    hidden: false,
  });

  return styles;
}

export interface ContrastPair {
  /** Le classi dell'elemento: è il nome con cui chi ripara lo cerca nel foglio. */
  where: string;
  /** Il testo che ci sta dentro, tagliato: serve a ritrovarlo sullo schermo. */
  text: string;
  ratio: number;
  foreground: Rgb;
  background: Rgb;
  opacity: number;
}

/** Il testo scritto direttamente dentro un elemento, senza quello dei figli. */
function ownText(element: Element): string {
  let text = "";
  for (const node of Array.from(element.childNodes)) {
    if (node.nodeType === 3) text += ` ${node.textContent ?? ""}`;
  }
  return text.replace(/\s+/g, " ").trim();
}

/**
 * Ogni accoppiata testo/sfondo del DOM passato, col suo rapporto. Chi chiama
 * decide la soglia: il divieto 6 la mette a 4,5:1 senza eccezioni per il corpo
 * grande, perché su questa tela non c'è testo che si possa perdere.
 */
export function contrastPairs(root: Element, sheet: Stylesheet): ContrastPair[] {
  const styles = styleTree(root, sheet);
  const pairs: ContrastPair[] = [];

  for (const [element, style] of styles) {
    if (style.hidden) continue;
    const text = ownText(element);
    if (text === "") continue;
    const foreground = composite(
      { ...style.color, a: style.color.a * style.opacity },
      style.backdrop,
    );
    pairs.push({
      where: String(element.className || element.tagName.toLowerCase()),
      text: text.slice(0, 40),
      ratio: contrastRatio(foreground, style.backdrop),
      foreground,
      background: style.backdrop,
      opacity: style.opacity,
    });
  }

  return pairs;
}

/** La soglia del divieto 6, in un posto solo. */
export const MINIMUM_RATIO = 4.5;

/** Le accoppiate che il divieto 6 non ammette, pronte da stampare. */
export function belowThreshold(pairs: ContrastPair[], minimum = MINIMUM_RATIO): string[] {
  return pairs
    .filter((pair) => pair.ratio < minimum)
    .map(
      (pair) =>
        `${pair.where} «${pair.text}» ${pair.ratio.toFixed(2)}:1 (opacità ${pair.opacity.toFixed(2)})`,
    );
}
