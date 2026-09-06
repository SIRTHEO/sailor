/**
 * **PROHIBITION 6, MEASURED BY A CHECK AND NOT BY A THROWAWAY TOOL.** No
 * text/ground pair under 4.5:1. The measure was taken once by hand, with a
 * script that died with its turn; the test this file makes possible goes red
 * when `--muted` is lightened by two characters, while `vitest`, `tsc` and the
 * identifier check stay green.
 *
 * **WHY AN ENGINE OF OUR OWN.** A real browser means a dependency and a
 * listening port, which answers `EPERM` inside the command perimeter — and a
 * check that skips itself when it cannot run is the hole being closed. So
 * `jsdom` draws the real DOM of the real components, and here is what jsdom
 * lacks: the cascade, custom-property inheritance, and `var()`. Its numbers
 * were compared against headless Chrome on the same page — same pairs, same
 * ratios to the hundredth — and against a checker that did not write it.
 *
 * **WHAT IT DOES NOT DO**, worth knowing before trusting it: no layout, no
 * pointer states, no pseudo-elements, and it skips the body of at-rules. A
 * skipped rule that declares a colour is counted, so writing one is said out
 * loud instead of passing in silence.
 */
export interface Rgb {
  r: number;
  g: number;
  b: number;
  /** 0 = wholly transparent, 1 = full. */
  a: number;
}

const TRANSPARENT: Rgb = { r: 0, g: 0, b: 0, a: 0 };

/** The last ground under the page: the one the browser paints by itself. */
export const CANVAS_WHITE: Rgb = { r: 255, g: 255, b: 255, a: 1 };

function channel(text: string, scale: number): number {
  const clean = text.trim();
  if (clean.endsWith("%")) return (Number.parseFloat(clean) / 100) * scale;
  return Number.parseFloat(clean);
}

/**
 * The forms this sheet really uses: `transparent`, hex at 3, 4, 6 or 8 digits,
 * and `rgb()`/`rgba()` with commas or with spaces and the slash. Everything
 * else comes back `null` — better not to know than to guess.
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

/** Lays `front` over `back`, which is assumed opaque. */
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

/** The WCAG contrast ratio between two opaque colours. */
export function contrastRatio(first: Rgb, second: Rgb): number {
  const a = luminance(first);
  const b = luminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// ── the stylesheet, read as rules ──────────────────────────────────────

export interface CssRule {
  selector: string;
  specificity: number;
  order: number;
  declarations: Array<[string, string]>;
}

export interface Stylesheet {
  rules: CssRule[];
  /**
   * How many colour declarations sit inside a skipped at-rule. It must stay
   * zero: if somebody puts a colour there, this engine does not see it and the
   * check would go blind in silence.
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

/**
 * **WHERE THE ROLES ARE DEFINED.** `:root` is the ground. A look a person pins
 * writes the same roles again under `[data-theme]`, and the answer the machine
 * gives writes them under a `:not()` of it — all three are role grounds, and
 * none of them is «a rule that paints something».
 */
export function isRoleGround(selector: string): boolean {
  return /^:root(:not\(\[data-theme="(?:dark|light)"\]\)|\[data-theme="(?:dark|light)"\])?$/.test(
    selector.trim(),
  );
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

/** Strips the comments without touching the rest. */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** A state this engine cannot draw: the rule is skipped. */
const STATEFUL = /::|:hover|:focus|:active|:disabled|:checked|:target|:visited|:enabled/i;

/** True if this declaration carries a colour, whatever it is named. */
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

/** Splits on a separator only outside parentheses, quotes and brackets. */
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
        if (isRoleGround(rule.selector)) {
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

// ── the cascade, inheritance and `var()` ───────────────────────────────

/** Resolves `var(--nome, ripiego)` all the way down, nested fallbacks and all. */
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
   * The declarations that win on this element, with `var()` already resolved.
   * Not colour alone: it is from here that `layout.test.tsx` reads the sizes and
   * line heights of a lane's heading, instead of copying them out.
   */
  declarations: Map<string, string>;
  color: Rgb;
  /** The effective ground under this element's text, already opaque. */
  backdrop: Rgb;
  /** The opacity accumulated from the root down to here. */
  opacity: number;
  hidden: boolean;
}

/**
 * The properties that count here. `color` is inherited, the others are not — the
 * only inheritance needed, because the ground is rebuilt by climbing back up.
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
  // Inline style wins over everything: this is how a lane's colour and the
  // tint of a tool's mark arrive.
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
 * Walks the drawn DOM and works out, for each element, the colours the browser
 * would give it. The ground composes on the way down: each element lays its own
 * background — its alpha multiplied by the accumulated opacity — over the ground
 * of whatever contains it.
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
  /** The element's classes: the name a repairer looks it up by in the sheet. */
  where: string;
  /** The text inside it, cut short: it serves to find it again on screen. */
  text: string;
  ratio: number;
  foreground: Rgb;
  background: Rgb;
  opacity: number;
}

/** The text written directly inside an element, without the children's. */
function ownText(element: Element): string {
  let text = "";
  for (const node of Array.from(element.childNodes)) {
    if (node.nodeType === 3) text += ` ${node.textContent ?? ""}`;
  }
  return text.replace(/\s+/g, " ").trim();
}

/**
 * Every text/ground pair of the DOM handed in, with its ratio. The caller sets
 * the threshold: prohibition 6 puts it at 4,5:1 with no exception for large
 * type, because on this canvas there is no text that may be allowed to get lost.
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

/** The threshold of prohibition 6, in one place only. */
export const MINIMUM_RATIO = 4.5;

/** The pairs prohibition 6 does not allow, ready to print. */
export function belowThreshold(pairs: ContrastPair[], minimum = MINIMUM_RATIO): string[] {
  return pairs
    .filter((pair) => pair.ratio < minimum)
    .map(
      (pair) =>
        `${pair.where} «${pair.text}» ${pair.ratio.toFixed(2)}:1 (opacità ${pair.opacity.toFixed(2)})`,
    );
}
