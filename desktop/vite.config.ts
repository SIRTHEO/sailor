import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// La porta è fissa perché il guscio nativo la apre per nome, non per scoperta.
// Diversa da quella di `sailor ui` (47831), che serve la pagina in sola lettura:
// finché le due esistono insieme non devono litigare.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5183,
    strictPort: true,
    // `ports.test.tsx` conta le porte sui **dieci file veri**, che stanno un
    // piano sopra la radice di questa app: senza questa riga il guardiano del
    // file system di Vite li nega e la prova non parte affatto.
    //
    // **SONO DUE CARTELLE PERCHÉ I FLUSSI VIVONO IN DUE POSTI**, e non è un
    // dettaglio di percorso: `dispatch-the-work` è **spedito dentro il binario**
    // (`crates/flow/system/`, incorporato con `include_str!`) perché le regole
    // di instradamento che viaggiano col prodotto lo nominano, e su un'altra
    // macchina la cartella `flows/` non esiste. Gli altri nove sono di questo
    // progetto. **La finestra li disegna allo stesso modo**, quindi la misura
    // che censisce le porte deve vederli tutti e due i posti: quando quel file
    // è passato nel binario, il censimento è sceso da 10 flussi a 9 e da 20
    // catene a 18 senza che nessuno lo volesse — la prova è diventata rossa, ed
    // era il modo giusto di accorgersene.
    //
    // Si aprono le **due cartelle nominate**, mai `..`: la radice intera
    // conterrebbe anche `target/` e le chiavi di chi lavora qui.
    // E `../i18n`, i due cataloghi: stanno nella radice del repo e non dentro
    // questa app perché sono **una cosa sola con due superfici** — i crate li
    // incorporano con `include_str!`, il bundler li impacchetta qui. Se
    // stessero in casa di una delle due, l'altra sarebbe ospite.
    fs: { allow: [".", "../flows", "../crates/flow/system", "../i18n"] },
  },
  // `SAILOR_LANG` arriva fino alla finestra, che altrimenti vedrebbe solo le
  // variabili con prefisso `VITE_`. In mancanza si parla inglese.
  envPrefix: ["VITE_", "SAILOR_"],
  build: { outDir: "dist", emptyOutDir: true },
  // Senza questo `vitest` restituisce una stringa vuota per ogni import di CSS,
  // `?raw` compreso: i controlli dei divieti leggerebbero un foglio vuoto e
  // sarebbero verdi per non aver guardato niente.
  test: { css: true },
});
