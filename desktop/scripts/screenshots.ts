/**
 * GLI OCCHI DEL FLUSSO: disegna la finestra, la fotografa, e ne scrive l'albero.
 *
 * Il contratto lo fissa il passo `schermo` dei flussi di design
 * (`~/.config/sailor/flows/`): uno script npm chiamato `screenshots` che avvii
 * il progetto, catturi a 375 e a 1440 pixel di larghezza, e salvi in
 * `design/screenshots/`. Chi cambia quel contratto cambia anche questo file.
 *
 * PERCHÉ DUE USCITE E NON UNA. Ogni scena produce un PNG **e** un albero di
 * accessibilità.
 *   · L'albero costa poche centinaia di token, dice i ruoli e i nomi, ed è
 *     deterministico: è quello che serve per sapere *cosa c'è*.
 *   · Il PNG costa molto di più e serve per la sola domanda a cui l'albero non
 *     può rispondere: *come appare*. Nessun albero dice che una pagina non ha
 *     gerarchia, perché la gerarchia è una relazione fra dimensioni, pesi e
 *     spazi — cioè esattamente ciò che l'albero butta via per essere leggero.
 * Su questa finestra c'è una terza ragione: la tela è fatta di corde, corsie e
 * posizioni. Sono geometria, e l'albero non porta la geometria — dice che ci
 * sono otto nodi, non che si accavallano.
 *
 * PERCHÉ SENZA IL GUSCIO NATIVO. Fuori da Tauri `insideTheWindow()` è falso e
 * la finestra disegna `SAMPLE`: una tela popolata, sempre la stessa, senza il
 * supervisor dietro. È il contrario di un limite — una scena che cambia a ogni
 * corsa non si può confrontare con quella di ieri.
 *
 * UNA SCENA CHE NON SI RAGGIUNGE NON SI INVENTA: finisce in `missing.txt` con
 * il motivo, e la corsa prosegue. Un giudizio visivo dato su un'immagine che
 * non esiste è peggio di nessun giudizio, perché nessuno va a ricontrollare.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser, type Page } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const outDir = join(root, "design", "screenshots");

/** Fissa in `vite.config.ts`, perché il guscio nativo la apre per nome. */
const PORT = 5183;
const URL = `http://localhost:${PORT}/`;

/** Le due larghezze che il contratto impone: il telefono e la scrivania. */
const WIDTHS = [
  { name: "375", width: 375, height: 812 },
  { name: "1440", width: 1440, height: 900 },
];

type Scene = {
  /** Il nome del file. Inglese, come ogni identificatore di questo albero. */
  name: string;
  /** Cosa questa scena serve a far vedere, per chi legge il giudizio. */
  what: string;
  /** Porta la finestra nello stato voluto. Se non ci riesce, alza. */
  reach: (page: Page) => Promise<void>;
};

/**
 * Apre una delle viste della barra in alto. Il nome si cerca sul testo del
 * pulsante e non su una classe: le classi si rinominano senza che nessuno se
 * ne accorga, il nome che una persona legge sullo schermo no — e se cambia,
 * è cambiato il prodotto e questa cattura *deve* accorgersene.
 */
async function openView(page: Page, label: string): Promise<void> {
  const tab = page.getByRole("button", { name: new RegExp(`^\\s*${label}`, "i") }).first();
  await tab.waitFor({ state: "visible", timeout: 5000 });
  await tab.click();
}

/**
 * Porta alla tela e sceglie il primo flusso della colonna.
 *
 * POI INQUADRA, E L'INQUADRATURA NON È UN DETTAGLIO DI CATTURA. Scegliendo un
 * flusso dalla colonna la tela resta ferma dov'era: i nodi esistono, sono nel
 * DOM, e stanno fuori dalla vista — si vedono solo nella minimappa. Chi
 * fotografa senza inquadrare fotografa una tela vuota e la chiama «il flusso a
 * fuoco»; chi la guarda giudica un vuoto che il prodotto non ha.
 *
 * Che poi sia il prodotto a doverlo fare da sé quando si sceglie un flusso è
 * una domanda vera, e sta nella cattura solo perché qui si è vista per prima.
 */
async function focusFirstFlow(page: Page): Promise<void> {
  await openView(page, "flussi");
  const first = page.locator(".rail__item").first();

  // Below the narrow threshold the flow list withdraws so the canvas survives,
  // and with it goes the only way to pick a flow. That is a product gap, not a
  // capture failure, and saying so is worth more than a timeout nobody reads.
  if (!(await first.isVisible().catch(() => false))) {
    throw new Error(
      "no way to choose a flow at this width: the flow column is withdrawn and " +
        "the top bar names the open flow without offering the others",
    );
  }

  await first.click();
  // Si aspetta un nodo, non un tempo fisso: un'attesa a orologio è il difetto
  // che fa passare una prova per il motivo sbagliato.
  await page.locator(".react-flow__node-step").first().waitFor({ timeout: 5000 });

  // Su schermo stretto il controllo può finire sotto la barra degli attrezzi.
  // Allora si fotografa senza inquadrare invece di far cadere la scena — ma la
  // differenza si legge nell'immagine, e chi giudica deve saperlo.
  try {
    const fitView = page.locator(".react-flow__controls-fitview");
    await fitView.click({ timeout: 3000 });
    // L'inquadratura è una transizione: si aspetta che un nodo sia davvero
    // nella vista, non che sia passato del tempo.
    await page.locator(".react-flow__node-step").first().waitFor({ state: "visible", timeout: 5000 });
  } catch {
    console.log("    · inquadratura non riuscita: la tela resta dov'era");
  }
}

const SCENES: Scene[] = [
  {
    name: "now",
    what: "la vista di apertura. Fuori dal guscio nativo il deposito non si legge, quindi questa è anche la scena del guasto — uno degli stati che nessuno guarda mai, e dove il default sopravvive più a lungo",
    reach: async () => {},
  },
  {
    name: "flows-canvas",
    what: "la tela dei flussi con la colonna: LA superficie del prodotto, quella che si disegna con oggetti JavaScript e che il lettore del foglio di stile non apre mai",
    reach: async (page) => {
      await openView(page, "flussi");
      await page.locator(".react-flow").waitFor({ state: "visible", timeout: 5000 });
    },
  },
  {
    name: "flow-in-focus",
    what: "un flusso scelto: la sua corsia, i suoi nodi e le corde fra loro — la geometria, che nessun albero di accessibilità porta",
    reach: focusFirstFlow,
  },
  {
    name: "step-selected",
    what: "un passo a fuoco e il pannello che lo descrive: il caso più denso di tutti",
    reach: async (page) => {
      await focusFirstFlow(page);
      // SI CLICCA UN NODO CHE STA DENTRO LA FINESTRA, non il primo del
      // documento e nemmeno il primo che lo strumento chiama «visibile».
      // «Visibile» per Playwright vuol dire che ha dimensioni e non è
      // nascosto: un nodo sei schermate a destra è visibile in quel senso e
      // non arriva sotto il puntatore mai. Su schermo stretto l'inquadratura
      // ne tiene dentro due su otto — e i sei fuori sono giusti che stiano
      // fuori: un flusso più largo dello schermo non ci sta, e non è un
      // difetto della finestra.
      //
      // Quindi lo si sceglie per geometria, come faceva chi guardava.
      const id = await page.evaluate(`(() => {
        var pane = document.querySelector(".react-flow");
        if (!pane) return null;
        var box = pane.getBoundingClientRect();
        var nodes = Array.prototype.slice.call(document.querySelectorAll(".react-flow__node-step"));
        for (var i = 0; i < nodes.length; i++) {
          var b = nodes[i].getBoundingClientRect();
          if (b.left >= box.left && b.right <= box.right && b.top >= box.top && b.bottom <= box.bottom) {
            return nodes[i].getAttribute("data-id");
          }
        }
        return null;
      })()`);
      if (id === null) throw new Error("nessun nodo sta per intero dentro la tela a questa larghezza");
      const node = page.locator(`.react-flow__node-step[data-id="${id}"]`);
      await node.click({ timeout: 5000 });
      await page.locator(".panel").waitFor({ state: "visible", timeout: 5000 });
    },
  },
  {
    name: "installed",
    what: "cosa questa macchina ha montato: una vista di soli dati, dove il ritmo verticale si giudica meglio che altrove",
    reach: async (page) => {
      await openView(page, "installato");
    },
  },
  {
    name: "history",
    what: "la storia delle corse: l'altra vista densa, e quella che invecchia peggio",
    reach: async (page) => {
      await openView(page, "storia");
    },
  },
];

/** Aspetta che vite risponda, invece di dormire un tempo indovinato. */
async function waitForVite(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(URL, { signal: AbortSignal.timeout(1000) });
      if (response.ok) return;
    } catch {
      // non è ancora in ascolto: si riprova
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`vite non risponde su ${URL} dopo ${timeoutMs}ms`);
}

async function main(): Promise<void> {
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });

  const missing: string[] = [];

  // Se qualcosa è già in ascolto sulla porta, è di qualcun altro: non lo si
  // uccide e non se ne avvia un secondo (`strictPort` fallirebbe comunque).
  let vite: ChildProcess | null = null;
  let alreadyServing = false;
  try {
    const response = await fetch(URL, { signal: AbortSignal.timeout(800) });
    alreadyServing = response.ok;
  } catch {
    alreadyServing = false;
  }

  if (alreadyServing) {
    console.log(`--- ${URL} risponde già: uso quello che c'è ---`);
  } else {
    console.log("--- avvio vite ---");
    vite = spawn("npm", ["run", "dev"], { cwd: root, stdio: "ignore" });
    await waitForVite(30_000);
  }

  let browser: Browser | null = null;
  try {
    // Si usa il Chrome installato invece di scaricare i browser di Playwright:
    // un download di centinaia di megabyte per fotografare tre scene è un
    // prezzo che questo albero non deve pagare.
    browser = await chromium.launch({ channel: "chrome" });

    for (const size of WIDTHS) {
      const context = await browser.newContext({
        viewport: { width: size.width, height: size.height },
        deviceScaleFactor: 2,
        // Il divieto sul movimento vale anche qui: una cattura che prende
        // un'animazione a metà non è confrontabile con quella di ieri.
        reducedMotion: "reduce",
      });
      const page = await context.newPage();
      await page.goto(URL, { waitUntil: "networkidle" });

      for (const scene of SCENES) {
        const stem = `${scene.name}-${size.name}`;
        try {
          await page.goto(URL, { waitUntil: "networkidle" });
          await scene.reach(page);

          await page.screenshot({
            path: join(outDir, `${stem}.png`),
            fullPage: false,
          });

          // L'albero: i ruoli e i nomi di ciò che c'è, in poche centinaia di
          // token invece che in un'immagine.
          const tree = await page.locator("body").ariaSnapshot();
          await writeFile(
            join(outDir, `${stem}.aria.txt`),
            `# ${scene.name} @ ${size.width}px — ${scene.what}\n\n${tree}\n`,
            "utf-8",
          );

          console.log(`  ✓ ${stem}`);
        } catch (error) {
          const why = error instanceof Error ? error.message : String(error);
          missing.push(`${stem}: ${why}`);
          console.log(`  ✖ ${stem} — non raggiunta: ${why}`);
        }
      }

      await context.close();
    }
  } finally {
    await browser?.close();
    vite?.kill();
  }

  // Ciò che non si è raggiunto si scrive, sempre: un elenco vuoto e un elenco
  // mai prodotto si leggono uguali, e si decide in modo opposto.
  await writeFile(
    join(outDir, "missing.txt"),
    missing.length === 0
      ? "nessuna scena mancante: tutte raggiunte e catturate.\n"
      : `${missing.length} scene non raggiunte:\n${missing.map((m) => `- ${m}`).join("\n")}\n`,
    "utf-8",
  );

  console.log(`\n--- ${outDir} ---`);
  if (missing.length > 0) {
    console.log(`${missing.length} scene mancanti: vedi missing.txt`);
    // Non si esce rossi: il passo `schermo` tollera il proprio fallimento, e
    // una cattura parziale vale più di nessuna cattura. Chi giudica legge
    // `missing.txt` e dichiara cosa non ha visto.
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
