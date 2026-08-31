import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// La porta è fissa perché il guscio nativo la apre per nome, non per scoperta.
// Diversa da quella di `sailor ui` (47831), che serve la pagina in sola lettura:
// finché le due esistono insieme non devono litigare.
export default defineConfig({
  plugins: [react()],
  server: { port: 5183, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true },
  // Senza questo `vitest` restituisce una stringa vuota per ogni import di CSS,
  // `?raw` compreso: i controlli dei divieti leggerebbero un foglio vuoto e
  // sarebbero verdi per non aver guardato niente.
  test: { css: true },
});
