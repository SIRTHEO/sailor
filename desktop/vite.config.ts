import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwind from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// The port is fixed because the native shell opens it by name, never by
// discovery. It differs from `sailor ui`'s (47831), which serves the read-only
// page: while the two exist side by side they must not fight over it.
export default defineConfig({
  plugins: [react(), tailwind()],
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
