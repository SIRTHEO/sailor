import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwind from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// La porta è fissa perché il guscio nativo la apre per nome, non per scoperta.
// Diversa da quella di `sailor ui` (47831), che serve la pagina in sola lettura:
// finché le due esistono insieme non devono litigare.
export default defineConfig({
  plugins: [react(), tailwind()],
  // `@/` è la radice dei sorgenti: è la convenzione che shadcn genera nei
  // propri componenti, e senza l'alias ogni file aggiunto va corretto a mano.
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    port: 5183,
    strictPort: true,
    // The tests read files that live above this app's root, and without these
    // lines Vite's file-system guard denies them and the test never starts.
    //
    // `realflows.ts` reads the flows shipped inside the binary from
    // `crates/flow/system/`, and their list from the `include_str!` lines of
    // `crates/flow/src/system.rs` — the source that owns it. It used to read
    // `flows/` too, and count what it found there: that count died the day the
    // workshop moved out, and `flows/` is not opened any more.
    //
    // Named directories, never `..`: the whole root would hold `target/` and
    // the keys of whoever works here. And `../i18n`, the two catalogues: they
    // sit at the repo root and not inside this app because they are one thing
    // with two surfaces — the crates embed them with `include_str!`, the
    // bundler packs them here. In either one's house, the other would be a
    // guest.
    fs: { allow: [".", "../crates/flow/system", "../crates/flow/src", "../i18n"] },
  },
  // `SAILOR_LANG` arriva fino alla finestra, che altrimenti vedrebbe solo le
  // variabili con prefisso `VITE_`. In mancanza si parla inglese.
  envPrefix: ["VITE_", "SAILOR_"],
  build: { outDir: "dist", emptyOutDir: true },
  // Senza questo `vitest` restituisce una stringa vuota per ogni import di CSS,
  // `?raw` compreso: i controlli dei divieti leggerebbero un foglio vuoto e
  // sarebbero verdi per non aver guardato niente.
  // The five-second default is a measure of the machine, not of the code: the
  // contrast checks paint the whole window and read every pair, and on a loaded
  // laptop one took 37 s against a limit of 5 and went red on a green tree.
  test: { css: true, testTimeout: 120_000 },
});
