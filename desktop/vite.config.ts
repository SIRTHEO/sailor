import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwind from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

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


// **THE BUILD CARRIES THE BRANDS THE DESCRIPTORS ASK FOR, AND NO OTHERS.**
// `simple-icons` holds 3461 marks, 4.8 MB of them. The descriptors are the only
// place a brand is named, so they are read here and each slug looked up once.
// A slug the catalogue lacks stops nothing: it reaches the screen as a
// monogram, which is what lets `openai` and `aws` still draw something.
const BRANDS = "virtual:brand-marks";
const BRANDS_RESOLVED = "\0brand-marks";

function theBrandMarks() {
  const here = (to: string) => fileURLToPath(new URL(to, import.meta.url));
  return {
    name: "the-brand-marks",
    resolveId: (id: string) => (id === BRANDS ? BRANDS_RESOLVED : null),
    load(id: string) {
      if (id !== BRANDS_RESOLVED) return null;
      const asked = new Set<string>();
      const folder = here("../crates/toolbox/descriptors");
      for (const file of readdirSync(folder).filter((name) => name.endsWith(".json"))) {
        const catalogue = JSON.parse(readFileSync(join(folder, file), "utf8")) as Record<string, unknown>;
        for (const entries of Object.values(catalogue)) {
          if (!Array.isArray(entries)) continue;
          for (const entry of entries) {
            const slug = (entry as { brand?: unknown }).brand;
            if (typeof slug === "string" && slug !== "") asked.add(slug);
          }
        }
      }

      const known = JSON.parse(
        readFileSync(here("./node_modules/simple-icons/data/simple-icons.json"), "utf8"),
      ) as { title: string; slug?: string; hex: string }[];
      const by = new Map(
        known.map((icon) => [icon.slug ?? icon.title.toLowerCase().replace(/[^a-z0-9]/g, ""), icon]),
      );

      // **THE ONE PLACE A MARK MAY BE KEPT BY HAND**, and only for a brand the
      // catalogue does not carry: upstream is read first, so the day it adds a
      // brand the copy here stops being used and a check asks for its removal.
      const ourOwn = (slug: string) => {
        const path = here(`./src/brands/${slug}.svg`);
        if (!existsSync(path)) return undefined;
        const drawing = readFileSync(path, "utf8");
        const drawn = /\sd="([^"]+)"/.exec(drawing)?.[1];
        const hex = /<svg[^>]*\sfill="(#[0-9a-fA-F]{6})"/.exec(drawing)?.[1];
        const title = /<title>([^<]+)<\/title>/.exec(drawing)?.[1];
        if (drawn === undefined || hex === undefined || title === undefined) {
          throw new Error(
            `${slug}.svg must carry one path, a fill of six hex digits on its <svg>, and a <title>`,
          );
        }
        return { title, hex: hex.toLowerCase(), path: drawn };
      };

      const marks: Record<string, { title: string; hex: string; path: string }> = {};
      for (const slug of [...asked].sort()) {
        const icon = by.get(slug);
        if (icon === undefined) {
          const mine = ourOwn(slug);
          if (mine !== undefined) marks[slug] = mine;
          continue;
        }
        const drawing = readFileSync(here(`./node_modules/simple-icons/icons/${slug}.svg`), "utf8");
        const path = /\sd="([^"]+)"/.exec(drawing)?.[1];
        if (path === undefined) continue;
        marks[slug] = { title: icon.title, hex: `#${icon.hex.toLowerCase()}`, path };
      }
      return `export default ${JSON.stringify(marks)};\n`;
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwind(), theLanguageLayer(process.env.SAILOR_LANG), theBrandMarks()],
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
