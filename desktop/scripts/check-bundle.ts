/**
 * What only a finished build can be measured on: how much is downloaded before
 * anything is on screen. It builds rather than trusting a `dist/` somebody
 * left behind — a gate reading yesterday's output measures nothing.
 */
import { execFileSync } from "node:child_process";
import { readdirSync, statSync } from "node:fs";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * **THESE CEILINGS ONLY EVER FALL.** Raising one is not a repair, it is the
 * gate being disarmed. Bytes on disk and not gzip: gzip is the network's
 * measure, bytes are the parser's, and the parser is what makes a window take
 * a second to answer.
 */
const BIGGEST_CHUNK_KB = 748;

/** Every woff2 the page can ask for. Seven subsets came to 219 kB in a tool
 *  whose whole vocabulary is Latin. */
const FONT_KB = 134;

/** A build with one chunk is the state this check exists to refuse. */
const FEWEST_CHUNKS = 4;

interface Asset {
  name: string;
  kilobytes: number;
}

function assetsOf(suffix: string): Asset[] {
  const where = join(root, "dist", "assets");
  return readdirSync(where)
    .filter((name) => name.endsWith(suffix))
    .map((name) => ({ name, kilobytes: statSync(join(where, name)).size / 1000 }))
    .sort((first, second) => second.kilobytes - first.kilobytes);
}

function kb(value: number): string {
  return `${value.toFixed(2)} kB`;
}

/**
 * **THE HEAVIEST LANGUAGE, NOT THE DEFAULT ONE.** The window carries one
 * catalogue, the one it was built to speak, so the ceiling measured on English
 * — which is also the smallest — would say nothing about the build a person
 * actually runs. Measured here: English 675 kB, Italian 758.
 */
function theHeaviestLanguage(): string {
  const here = new URL("../../i18n/", import.meta.url);
  const weighed = readdirSync(here)
    .filter((name) => name.endsWith(".json"))
    .map((name) => [name.replace(/\.json$/, ""), statSync(new URL(name, here)).size] as const)
    .sort((one, other) => other[1] - one[1]);
  return weighed[0]?.[0] ?? "";
}

const spoken = theHeaviestLanguage();
console.log(`the build speaks «${spoken}», the heaviest catalogue there is`);
execFileSync("npx", ["vite", "build"], {
  cwd: root,
  stdio: "inherit",
  env: { ...process.env, SAILOR_LANG: spoken },
});

const chunks = assetsOf(".js");
const fonts = assetsOf(".woff2");
const fontTotal = fonts.reduce((sum, font) => sum + font.kilobytes, 0);

console.log("\nthe chunks, heaviest first:");
for (const chunk of chunks) console.log(`  ${kb(chunk.kilobytes).padStart(10)}  ${chunk.name}`);
console.log(`\n  ${kb(fontTotal)} of woff2 across ${fonts.length} subsets`);

const complaints: string[] = [];

if (chunks.length < FEWEST_CHUNKS) {
  complaints.push(
    `${chunks.length} chunks, and the build is meant to carry at least ${FEWEST_CHUNKS}: ` +
      "a section that stopped being a dynamic import is a section everybody downloads.",
  );
}

const biggest = chunks[0];
if (biggest !== undefined && biggest.kilobytes > BIGGEST_CHUNK_KB) {
  complaints.push(
    `${biggest.name} is ${kb(biggest.kilobytes)} against a ceiling of ${BIGGEST_CHUNK_KB} kB. ` +
      "Move a section behind `React.lazy`, or say in the commit why the ceiling rises.",
  );
}

if (fontTotal > FONT_KB) {
  complaints.push(
    `${kb(fontTotal)} of woff2 against a ceiling of ${FONT_KB} kB, across ${fonts.length} subsets: ` +
      fonts.map((font) => font.name).join(", "),
  );
}

// The entry is the only script `index.html` names; a `modulepreload` beside it
// is a chunk fetched before the first paint whatever the import looked like.
const page = readFileSync(join(root, "dist", "index.html"), "utf8");
const preloaded = [...page.matchAll(/rel="modulepreload"[^>]*href="([^"]+)"/g)].map(
  (found) => found[1],
);
if (preloaded.length > 0) {
  console.log(`\n  preloaded beside the entry: ${preloaded.join(", ")}`);
}

if (complaints.length > 0) {
  console.error("\nthe bundle grew:");
  for (const complaint of complaints) console.error(`  ${complaint}`);
  process.exit(1);
}

console.log(
  `\nthe heaviest chunk is ${kb(biggest?.kilobytes ?? 0)} under a ceiling of ${BIGGEST_CHUNK_KB} kB, ` +
    `and the fonts ${kb(fontTotal)} under ${FONT_KB} kB.`,
);
