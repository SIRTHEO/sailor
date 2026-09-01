import { describe, expect, test } from "vitest";
import { CATALOGUES } from "./i18n";

/**
 * **NO SENTENCE THE USER READS OUTSIDE THE CATALOGUE.** One written into a
 * component cannot be published in the other language, and nothing turns red
 * when a new one appears, so this counts them and the count only falls. The
 * engine's strings are counted apart: one number over both would hide which.
 */

const sources = import.meta.glob("./*.{ts,tsx}", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

/**
 * Words a sentence cannot stand up without and that are **not valid English**.
 * Missing on purpose: `come`, `con`, `per`, `la`, `del` — they exist in both
 * and would accuse English. The declared price: a short Italian label that uses
 * none of them is not counted, so **this number is a floor**, never the total.
 */
const ITALIAN = new Set([
  "che", "non", "della", "delle", "degli", "nella", "nelle", "questo", "questa",
  "quello", "quella", "perché", "perche", "cioè", "cioe", "invece", "quindi",
  "anche", "essere", "senza", "più", "piu", "già", "gia", "sono", "dove",
  "quando", "sulla", "dalla", "dello", "il", "lo", "le", "gli", "un", "una",
  "nel", "dei", "alla", "allo", "alle", "sul", "sui", "dal", "dalle", "questi",
  "queste", "quali", "quale", "ogni", "solo", "ancora", "adesso", "prima",
  "dopo", "flusso", "flussi", "passo", "passi", "corsa", "corse", "scegli",
  "crea", "creane", "nuovo", "nuova", "aggiungi", "salva", "annulla", "chiudi",
  "apri", "registrati", "vuoto", "vuota", "niente", "nessun", "nessuna",
  "errore", "avvia", "ferma", "modifica", "elimina", "cerca", "carica",
  "deposito", "motore", "guscio", "macchina", "tela", "colonna", "barra",
  "pannello", "attesa", "verifica",
]);

export function looksItalian(text: string): boolean {
  const words = text.toLowerCase().match(/[a-zà-ÿ']+/g) ?? [];
  return words.some((word) => ITALIAN.has(word));
}

/** The lines that are comment, which the Rust ratchet already watches. */
function commentLines(lines: string[]): Set<number> {
  const out = new Set<number>();
  let inBlock = false;
  lines.forEach((line, i) => {
    const t = line.trimStart();
    if (inBlock) {
      out.add(i);
      if (t.includes("*/")) inBlock = false;
      return;
    }
    if (t.startsWith("//")) {
      out.add(i);
      return;
    }
    if (t.startsWith("/*") || t.startsWith("{/*")) {
      out.add(i);
      if (!t.slice(2).includes("*/")) inBlock = true;
    }
  });
  return out;
}

const LITERAL = /"([^"\n]{2,})"|'([^'\n]{2,})'|`([^`\n]{2,})`/g;

/**
 * A line that puts Italian on screen: JSX text, or a literal that is prose.
 * Paths, selectors and keys are skipped by shape — a slash, a leading dot, an
 * underscore — because a name is not a sentence even when it reads like one.
 */
function saysItalian(line: string): string | null {
  const bare = line.trim();
  if (!bare) return null;
  const withoutTags = bare.replace(/<[^>]*>/g, "").trim();
  if (withoutTags && !/[{};=]/.test(withoutTags) && looksItalian(withoutTags)) return withoutTags;
  for (const match of bare.matchAll(LITERAL)) {
    const text = match[1] ?? match[2] ?? match[3];
    if (text.includes("/") || text.startsWith(".") || text.includes("_")) continue;
    if (looksItalian(text)) return text;
  }
  return null;
}

export function loose(name: string, text: string): string[] {
  const lines = text.split("\n");
  const comments = commentLines(lines);
  const found: string[] = [];
  lines.forEach((line, i) => {
    if (comments.has(i)) return;
    const said = saysItalian(line);
    if (said !== null) found.push(`${name}:${String(i + 1)}  ${said.slice(0, 70)}`);
  });
  return found;
}

/**
 * How many lines still write Italian into a component instead of a key. **It
 * can only fall**: lowering it is the repair, raising it means a sentence was
 * written where it cannot be published.
 */
const LOOSE_TODAY = 133;

function everythingLoose(): string[] {
  const found: string[] = [];
  for (const [path, text] of Object.entries(sources)) {
    const name = path.replace("./", "");
    if (name.includes(".test.") || name === "i18n.ts") continue;
    found.push(...loose(name, text));
  }
  return found;
}

describe("what a person reads comes from the catalogue", () => {
  test("THE SENTENCES OUTSIDE THE CATALOGUE ONLY GET FEWER", () => {
    const loose = everythingLoose();
    expect(
      loose.length,
      `${String(loose.length)} righe scrivono italiano dentro un componente ` +
        `(dichiarate ${String(LOOSE_TODAY)}). L'elenco non dice quali siano ` +
        `le nuove — è in ordine di file — quindi guarda il tuo diff. ` +
        `Le ultime dieci trovate:\n${loose.slice(-10).join("\n")}`,
    ).toBeLessThanOrEqual(LOOSE_TODAY);
  });

  /**
   * **THE OTHER SIDE OF THE RATCHET.** A count that only has a ceiling trusts
   * the ceiling, and a ceiling is a number in a file that a merge can raise
   * with no conflict. A seed far above the tree is a seed nobody re-measured.
   */
  test("AND THE SEED STILL DESCRIBES THE TREE", () => {
    const measured = everythingLoose().length;
    expect(
      LOOSE_TODAY,
      `il seme dice ${String(LOOSE_TODAY)}, la finestra ne ha ${String(measured)}: ` +
        `il numero da scrivere è ${String(measured)}`,
    ).toBeLessThanOrEqual(measured);
  });

  /**
   * **CHI MISURA VA MISURATO.** If `looksItalian` or `saysItalian` stopped
   * seeing, the count would collapse to zero and the ratchet above would stay
   * green for ever, because a fall is what it is built to allow.
   */
  test("the measure can still see what it counts", () => {
    expect(looksItalian("Scegli un flusso nella colonna")).toBe(true);
    expect(looksItalian("Choose a flow in the column")).toBe(false);
    expect(saysItalian(`  <p>Il motore non risponde.</p>`)).not.toBeNull();
    expect(saysItalian(`  const path = "src/una/cartella";`)).toBeNull();
    expect(everythingLoose().length).toBeGreaterThan(0);
    // A comment is not copy: it belongs to the Rust ratchet, and counting it
    // here is what made the first hand measure say 253 instead of 133 — the
    // same `{/* … */}` blind spot that hid 256 lines from `is_comment`.
    expect(loose("finto.tsx", `// il motore non risponde\n{/* la tela è vuota */}`)).toEqual([]);
  });

  test("the catalogue itself is not counted, or the repair would raise the number", () => {
    const inCatalogue = Object.values(CATALOGUES.it).filter(looksItalian).length;
    expect(inCatalogue).toBeGreaterThan(0);
    expect(everythingLoose().join("\n")).not.toContain("i18n.ts:");
  });
});
