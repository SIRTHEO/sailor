import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwind from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { readFileSync } from "node:fs";

// **THE WINDOW CARRIES ONE CATALOGUE, NOT EVERY LANGUAGE THERE IS.** Both
// shipped in every build: 201 kB of the chunk that loads before anything is
// drawn, of which one was ever read. English is filled in under the spoken
// language here, so what ships is the answer and not the two halves of it.
const ASKED = "virtual:language-layer";
const RESOLVED = "\0language-layer";

function theLanguageLayer(spoken: string | undefined) {
  const wanted = (spoken ?? "").trim().toLowerCase().split(/[-_]/)[0];
  const read = (lang: string) =>
    JSON.parse(readFileSync(fileURLToPath(new URL(`../i18n/${lang}.json`, import.meta.url)), "utf8")) as Record<string, string>;
  return {
    name: "the-language-layer",
    resolveId: (id: string) => (id === ASKED ? RESOLVED : null),
    load(id: string) {
      if (id !== RESOLVED) return null;
      const source = read("en");
      const whole = wanted === "" || wanted === "en" ? source : { ...source, ...read(wanted) };
      return `export const SPOKEN = ${JSON.stringify(wanted || "en")};\n` +
        `export default ${JSON.stringify(whole)};\n`;
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwind(), theLanguageLayer(process.env.SAILOR_LANG)],
  // `@/` is the source root: the convention shadcn writes into its own
  // components, and without the alias every file added has to be fixed by hand.
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    port: 5183,
    strictPort: true,
    // Named directories, never `..`, which would hold `target/` and the keys
    // of whoever works here. **THE GUARD BITES ONLY UNDER JSDOM**: two tests
    // read crates on no list and passed, being in the node environment.
    fs: {
      allow: [
        ".",
        "../crates/flow/system",
        "../crates/flow/src",
        "../crates/ledger/src",
        "../crates/ui/src",
        "../i18n",
      ],
    },
  },
  // `SAILOR_LANG` reaches the window, which otherwise sees only the variables
  // prefixed `VITE_`. With none set, the language is English.
  envPrefix: ["VITE_", "SAILOR_"],
  build: { outDir: "dist", emptyOutDir: true },
  // Without this `vitest` answers an empty string to every CSS import, `?raw`
  // included, and the checks on what is forbidden are green for having read an
  // empty sheet. The five-second timeout measures the machine, not the code:
  // the contrast checks paint the whole window, and on a loaded laptop one
  // took 37 s against a limit of 5 and went red on a green tree.
  test: { css: true, testTimeout: 120_000 },
});
