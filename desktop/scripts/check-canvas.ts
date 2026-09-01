/**
 * **CIÒ CHE SI MISURA SOLO SU UNA SUPERFICIE DISEGNATA.**
 *
 * `stylesheet.test.ts` legge il foglio e trova le cause: una colonna rigida
 * senza via d'uscita, un carattere fuori dai ruoli. `contrast.test.tsx`
 * cammina il DOM calcolato e pesa le accoppiate. Nessuno dei due sa quanto è
 * larga una colonna in una finestra vera, né se ciò che c'è nel documento sia
 * dentro l'inquadratura: sono proprietà della geometria, e la geometria esiste
 * solo quando qualcosa disegna.
 *
 * Questo controllo apre un browser vero e misura le conseguenze. Gira a parte
 * da `npm test` — che deve restare di tre secondi e senza browser — e si
 * chiama con `npm run check:canvas`.
 *
 * **DUE COSE, E TUTTE E DUE SONO STATE ROSSE il 01/09/2026.**
 *
 * 1. LA TELA NON VA A ZERO. A 375 pixel colonna e pannello sommavano 520 e la
 *    tela — la superficie principale di questo prodotto — restava larga zero.
 *    La causa è riparata nel foglio (divieto 11); questo ne guarda l'effetto,
 *    che è l'unico posto dove si vedrebbe tornare per un'altra strada.
 *
 * 2. LA TELA INQUADRA CIÒ CHE CONTIENE. Aprendo la vista dei flussi c'erano
 *    otto nodi nel documento e **zero dentro la vista**, a ogni larghezza: chi
 *    apriva «FLUSSI» vedeva una tela vuota e i propri nodi solo come macchie
 *    nella minimappa. Il `fitView` iniziale gira al primo disegno, quando i
 *    nodi non sono ancora arrivati — entrano un giro dopo — e nessuno
 *    reinquadrava più.
 *
 * Questo file NON prova che la finestra sia bella: prova che c'è. Il giudizio
 * sull'aspetto lo dà chi guarda le immagini di `npm run screenshots`.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const URL = "http://localhost:5183/";

/** Le larghezze provate: le due del contratto di cattura, più le due soglie. */
const WIDTHS = [375, 760, 1100, 1440];

/**
 * Misurato dentro la pagina. Scritto in ES5 e senza funzioni nominate: lo
 * strumento che compila questo file inserisce un aiuto per i nomi che dentro
 * `evaluate` non esiste, e la misura muore con «__name is not defined».
 */
const MEASURE = `(() => {
  var width = function (selector) {
    var el = document.querySelector(selector);
    return el ? Math.round(el.getBoundingClientRect().width) : null;
  };
  var nodes = Array.prototype.slice.call(document.querySelectorAll(".react-flow__node-step"));
  var paneEl = document.querySelector(".react-flow");
  var pane = paneEl ? paneEl.getBoundingClientRect() : null;
  var inside = 0;
  if (pane) {
    for (var i = 0; i < nodes.length; i++) {
      var b = nodes[i].getBoundingClientRect();
      if (b.left < pane.right && b.right > pane.left && b.top < pane.bottom && b.bottom > pane.top) inside++;
    }
  }
  // Chi c'e' DAVVERO sotto il puntatore, al centro della tela. Una larghezza
  // non basta: un pannello che galleggia lascia la tela larga e la copre.
  var reachable = null;
  if (pane) {
    var hit = document.elementFromPoint(pane.left + pane.width / 2, pane.top + pane.height / 2);
    reachable = hit ? (hit.closest(".panel") ? "pannello" : "tela") : "niente";
  }
  var panelEl = document.querySelector(".panel");
  var panelEmpty = !!(panelEl && panelEl.querySelector(".panel__empty"));
  return { rail: width(".rail"), canvas: width(".react-flow"), panel: width(".panel"),
           nodes: nodes.length, inView: inside, reachable: reachable, panelEmpty: panelEmpty };
})()`;

async function serving(): Promise<boolean> {
  try {
    return (await fetch(URL, { signal: AbortSignal.timeout(800) })).ok;
  } catch {
    return false;
  }
}

async function main(): Promise<void> {
  let vite: ChildProcess | null = null;
  if (await serving()) {
    console.log(`--- ${URL} risponde già: uso quello che c'è ---`);
  } else {
    console.log("--- avvio vite ---");
    vite = spawn("npm", ["run", "dev"], { cwd: root, stdio: "ignore" });
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && !(await serving())) {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    if (!(await serving())) throw new Error(`vite non risponde su ${URL}`);
  }

  const failures: string[] = [];
  let browser: Browser | null = null;

  try {
    browser = await chromium.launch({ channel: "chrome" });

    for (const width of WIDTHS) {
      const context = await browser.newContext({
        viewport: { width, height: 812 },
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      await page.goto(URL, { waitUntil: "networkidle" });
      await page.getByRole("button", { name: /^\s*flussi/i }).first().click();
      await page.locator(".react-flow__node-step").first().waitFor({ timeout: 8000 });
      // L'inquadratura è una transizione dichiarata: le si lascia finire, poi
      // si misura dove sono finite le cose.
      await page.waitForTimeout(900);

      const m = (await page.evaluate(MEASURE)) as {
        rail: number | null;
        canvas: number | null;
        panel: number | null;
        nodes: number;
        inView: number;
        reachable: string | null;
        panelEmpty: boolean;
      };

      const line =
        `${String(width).padStart(5)}px  colonna ${String(m.rail).padStart(3)} · ` +
        `tela ${String(m.canvas).padStart(4)} · pannello ${String(m.panel).padStart(3)}   ` +
        `nodi in vista: ${m.inView}/${m.nodes}`;

      // 1. la tela esiste. La soglia non è zero: una tela di trenta pixel non è
      //    una tela, è un residuo di calcolo che nessuno guarderebbe.
      const canvasOk = (m.canvas ?? 0) >= 120;
      // 2. la tela inquadra ciò che contiene. Non «tutti»: un flusso più largo
      //    dello schermo non ci sta, ed è giusto così. Ma zero su otto vuol
      //    dire che chi apre non vede niente.
      const framedOk = m.nodes === 0 || m.inView > 0;
      // 3. la tela si può toccare. Una larghezza non basta: sotto la soglia
      //    stretta il pannello galleggia SOPRA la tela, e se galleggia anche
      //    quando non ha niente da dire copre ciò che copre per niente. È il
      //    difetto che la riparazione del divieto 11 ha creato, ed è stato
      //    trovato dallo strumento di cattura — un clic su un nodo che non
      //    arrivava mai.
      const reachableOk = !m.panelEmpty || m.reachable !== "pannello";

      console.log(`${canvasOk && framedOk && reachableOk ? "  ✓" : "  ✖"} ${line}   al centro: ${m.reachable}`);
      if (!canvasOk) failures.push(`${width}px: la tela è larga ${m.canvas}px — sotto i 120 non è una tela`);
      if (!framedOk) failures.push(`${width}px: ${m.nodes} nodi nel documento e nessuno dentro la vista`);
      if (!reachableOk) failures.push(`${width}px: al centro della tela c'è il pannello, e il pannello è vuoto`);

      await context.close();
    }
  } finally {
    await browser?.close();
    vite?.kill();
  }

  if (failures.length > 0) {
    console.error(`\n✖ ${failures.length} guasti:`);
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log("\n✓ la tela c'è a ogni larghezza, e inquadra ciò che contiene.");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
