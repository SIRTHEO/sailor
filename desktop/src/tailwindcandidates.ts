/**
 * **IS THIS CLASS A UTILITY, OR A TYPO?** `justify-strat` produces nothing, and
 * only Tailwind can say so. It is asked.
 */
import { compile } from "tailwindcss";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

/** Where utilities come from. This window's own sheet is not among them. */
const UTILITY_SHEETS = '@import "tailwindcss";\n@import "tw-animate-css";\n';

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
