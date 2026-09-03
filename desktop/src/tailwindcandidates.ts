/**
 * **IS THIS CLASS A UTILITY, OR A TYPO?** `justify-strat` produces nothing, and
 * only Tailwind can say so. It is asked.
 */
import { compile } from "tailwindcss";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import stylesheetSource from "./styles.css?raw";

/**
 * The `@theme` block of this window's sheet: it is what names `foreground`,
 * `popover` and the rest, so without it `bg-foreground` compiles to nothing
 * and reads as a typo.
 */
function themeOf(source: string): string {
  // Comments first: the sheet mentions `@theme` in prose above the block, and
  // the scan below would take the sentence for the rule.
  const sheet = source.replace(/\/\*[\s\S]*?\*\//g, "");
  const start = sheet.indexOf("@theme");
  if (start < 0) return "";
  let depth = 0;
  for (let at = sheet.indexOf("{", start); at < sheet.length; at += 1) {
    if (sheet[at] === "{") depth += 1;
    if (sheet[at] === "}") {
      depth -= 1;
      if (depth === 0) return sheet.slice(start, at + 1);
    }
  }
  return "";
}

/** Where utilities come from: Tailwind, its animations, and our own roles. */
const UTILITY_SHEETS =
  `@import "tailwindcss";\n@import "tw-animate-css";\n${themeOf(stylesheetSource)}\n`;

/** `require.resolve` cannot reach a sheet published under `style`. */
function sheetOf(id: string, base: string): string {
  if (id.startsWith(".")) return resolve(base, id);
  const manifest = resolve(process.cwd(), "node_modules", id, "package.json");
  const entries = JSON.parse(readFileSync(manifest, "utf8")).exports ?? {};
  const entry = entries["."]?.style ?? entries["."]?.default ?? "./index.css";
  return resolve(dirname(manifest), entry);
}

async function loadStylesheet(id: string, base: string) {
  const path = sheetOf(id, base);
  return { path, base: dirname(path), content: readFileSync(path, "utf8") };
}

/** How Tailwind writes a class: `p-1.5` becomes `.p-1\\.5`. */
function selectorOf(name: string): string {
  return `.${name.replace(/[^A-Za-z0-9_-]/g, (character) => `\\${character}`)}`;
}

/** The subset of `names` Tailwind turns into a rule, in one pass: the
 *  compiler accumulates, so one at a time would answer for the ones before. */
export async function utilities(names: Iterable<string>): Promise<Set<string>> {
  const compiler = await compile(UTILITY_SHEETS, { base: process.cwd(), loadStylesheet });
  const wanted = [...names];
  const css = compiler.build(wanted);
  return new Set(wanted.filter((name) => css.includes(selectorOf(name))));
}
