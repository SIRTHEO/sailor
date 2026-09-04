/**
 * The eyes: draws the window, photographs it, and writes down its tree.
 *
 * Contract, set by the `schermo` step of the design flows: an npm script named
 * `screenshots` that starts the project, captures at 375 and 1440 pixels, and
 * saves into `design/screenshots/`.
 *
 * Two outputs per scene, and they are not interchangeable. The tree costs a few
 * hundred tokens and says what is there: roles, names, which cords join which
 * nodes. The image costs far more and answers the one question the tree cannot:
 * how it looks. No tree says a page has no hierarchy, because hierarchy is a
 * relation between sizes, weights and spacing — exactly what the tree drops to
 * stay small. On a canvas there is a third reason: lanes, cords and positions
 * are geometry, and a tree says there are eight nodes, not that they overlap.
 *
 * Outside the native shell the window draws `SAMPLE`: a populated canvas,
 * always the same, with no supervisor behind it. A scene that changed every run
 * could not be compared with yesterday's.
 *
 * A scene that cannot be reached is not invented: it lands in `missing.txt`
 * with the reason.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser, type Page } from "playwright";
import { PLACES, type Section } from "../src/places";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const outDir = join(root, "design", "screenshots");

/** Fixed in `vite.config.ts`: the native shell opens it by name. */
const PORT = 5183;
const URL = `http://localhost:${PORT}/`;

/** The two widths the contract sets: the phone and the desk. */
const WIDTHS = [
  { name: "375", width: 375, height: 812 },
  { name: "1440", width: 1440, height: 900 },
];

type Scene = {
  /** The file name. */
  name: string;
  /** What this scene exists to show, for whoever reads the judgement. */
  what: string;
  /** Brings the window to the wanted state, or throws. */
  reach: (page: Page) => Promise<void>;
};

/**
 * Opens a place by the name the product gives it, not by a word written here:
 * copied once, the names went stale and the capture went blind in silence.
 */
async function openPlace(page: Page, id: Section): Promise<void> {
  const place = PLACES.find((one) => one.id === id);
  if (!place) throw new Error(`no place «${id}»: the product does not have it`);
  const tab = page.getByRole("button", { name: new RegExp(`^\\s*${place.name}`, "i") }).first();
  await tab.waitFor({ state: "visible", timeout: 5000 });
  await tab.click();
}

/** A row of the machine's ground, by the name the column shows. */
async function openMachineRow(page: Page, label: string): Promise<void> {
  const row = page.locator(".world__global", { hasText: label }).first();
  await row.waitFor({ state: "visible", timeout: 5000 });
  await row.click();
}

/** The mark a refusal carries when the state is not in the product at all. */
const A_GAP_IN_THE_PRODUCT = "product gap: ";

/**
 * **THE FRAME IS ANIMATED, SO IT IS WAITED FOR, NOT ASSUMED.** The canvas takes
 * three hundred milliseconds to settle on a flow; a geometry read while it
 * moves names a node that is somewhere else by the time the pointer arrives.
 */
async function framed(page: Page): Promise<void> {
  const where = async () =>
    await page.evaluate(
      `(() => { var vp = document.querySelector(".react-flow__viewport");
                return vp ? vp.style.transform : ""; })()`,
    );
  let last = await where();
  for (let tries = 0; tries < 20; tries += 1) {
    await page.waitForTimeout(100);
    const now = await where();
    if (now === last && now !== "") return;
    last = now;
  }
}

/**
 * Reaches the canvas and focuses the first flow, then frames it.
 *
 * Framing is not a capture detail: whoever photographs without it photographs
 * an empty canvas and calls it "the flow in focus", and whoever looks judges an
 * emptiness the product does not have.
 */
async function focusFirstFlow(page: Page): Promise<void> {
  await openPlace(page, "board");
  const first = page.locator(".rail__item").first();

  // Below the narrow threshold the flow list withdraws so the canvas survives,
  // and with it goes the only way to pick a flow: a product gap, not a capture
  // failure — and the difference is what decides the exit code below.
  if (!(await first.isVisible().catch(() => false))) {
    throw new Error(
      `${A_GAP_IN_THE_PRODUCT}no way to choose a flow at this width: the flow ` +
        "column is withdrawn and the top bar names the open flow without " +
        "offering the others",
    );
  }

  await first.click();
  // Wait for a node, not for a duration: a clock wait passes for the wrong
  // reason.
  await page.locator(".react-flow__node-step").first().waitFor({ timeout: 5000 });

  // On a narrow screen the control can end up under the toolbar. Then the
  // scene is photographed unframed rather than dropped, and says so.
  try {
    const fitView = page.locator(".react-flow__controls-fitview");
    await fitView.click({ timeout: 3000 });
    // Wait for a node to actually be in view, not for time to pass.
    await page.locator(".react-flow__node-step").first().waitFor({ state: "visible", timeout: 5000 });
  } catch {
    console.log("    · framing failed: the canvas stays where it was");
  }
  await framed(page);
}

const SCENES: Scene[] = [
  {
    name: "now",
    what: "the opening view. Outside the native shell the ledger cannot be read, so this doubles as the failure scene — one of the states nobody looks at, where the default survives longest",
    reach: async () => {},
  },
  {
    name: "flows-canvas",
    what: "the flow canvas with its rail: THE surface of this product, drawn with JavaScript objects and never opened by whatever reads the stylesheet",
    reach: async (page) => {
      await openPlace(page, "board");
      await page.locator(".react-flow").waitFor({ state: "visible", timeout: 5000 });
    },
  },
  {
    name: "flow-in-focus",
    what: "a flow in focus: its lane, its nodes and the cords between them — the geometry no accessibility tree carries",
    reach: focusFirstFlow,
  },
  {
    name: "step-selected",
    what: "a selected step and the panel describing it: the densest case",
    reach: async (page) => {
      await focusFirstFlow(page);
      // "Visible" to the driver means sized and not hidden: a node six screens
      // to the right is visible in that sense and never reaches the pointer.
      // So the node is chosen by geometry, the way whoever looks chooses it.
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
      if (id === null) throw new Error("no node fits entirely inside the canvas at this width");
      const node = page.locator(`.react-flow__node-step[data-id="${id}"]`);
      // The node fits inside the canvas and is still unreachable: at 375 the
      // toolbox lies over the lower-left of the paper, and what is under it
      // takes no click. A gap in the product, not a blind capture — and told
      // apart from one by the driver's own words.
      await node.click({ timeout: 5000 }).catch((trouble: unknown) => {
        const said = String(trouble);
        if (!said.includes("intercepts pointer events")) throw trouble;
        throw new Error(
          `${A_GAP_IN_THE_PRODUCT}the toolbox lies over the canvas at this ` +
            "width, and a step under it cannot be chosen",
        );
      });
      await page.locator(".panel").waitFor({ state: "visible", timeout: 5000 });
    },
  },
  {
    name: "installed",
    what: "what this machine has installed: a data-only view, where vertical rhythm shows more than elsewhere",
    reach: async (page) => {
      // One click: the machine's places are rows of the one column now.
      await openMachineRow(page, "Equipment");
    },
  },
  {
    name: "history",
    what: "the run history: the other dense view, and the one that ages worst",
    reach: async (page) => {
      await openPlace(page, "memory");
    },
  },
];

/** Waits for vite to answer, rather than sleeping a guessed duration. */
async function waitForVite(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(URL, { signal: AbortSignal.timeout(1000) });
      if (response.ok) return;
    } catch {
      // not listening yet
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`vite does not answer on ${URL} after ${timeoutMs}ms`);
}

async function main(): Promise<void> {
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });

  const missing: string[] = [];

  // Something already on the port belongs to someone else: it is not killed,
  // and a second one would fail `strictPort` anyway.
  let vite: ChildProcess | null = null;
  let alreadyServing = false;
  try {
    const response = await fetch(URL, { signal: AbortSignal.timeout(800) });
    alreadyServing = response.ok;
  } catch {
    alreadyServing = false;
  }

  if (alreadyServing) {
    console.log(`--- ${URL} already answers: using it ---`);
  } else {
    console.log("--- starting vite ---");
    vite = spawn("npm", ["run", "dev"], { cwd: root, stdio: "ignore" });
    await waitForVite(30_000);
  }

  let browser: Browser | null = null;
  try {
    // The installed Chrome, not a download: hundreds of megabytes to
    // photograph a few scenes is a price this tree need not pay.
    browser = await chromium.launch({ channel: "chrome" });

    for (const size of WIDTHS) {
      const context = await browser.newContext({
        viewport: { width: size.width, height: size.height },
        deviceScaleFactor: 2,
        // A capture that catches an animation halfway cannot be compared
        // with yesterday's.
        reducedMotion: "reduce",
        // The direction's ground is night. Playwright emulates day unless
        // told, so without this line the eyes photograph the second scheme.
        colorScheme: "dark",
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

          // The tree: roles and names, in a few hundred tokens.
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
          console.log(`  ✖ ${stem} — not reached: ${why}`);
        }
      }

      await context.close();
    }
  } finally {
    await browser?.close();
    vite?.kill();
  }

  // What was not reached is always written: an empty list and a list never
  // produced read the same, and are decided on oppositely.
  await writeFile(
    join(outDir, "missing.txt"),
    missing.length === 0
      ? "no scene missing: all reached and captured.\n"
      : `${missing.length} scenes not reached:\n${missing.map((m) => `- ${m}`).join("\n")}\n`,
    "utf-8",
  );

  console.log(`\n--- ${outDir} ---`);
  if (missing.length === 0) return;
  console.log(`${missing.length} scenes missing: see missing.txt`);
  // A tolerance without a ceiling is not a tolerance: ten scenes out of eleven
  // went missing for days and this exited zero. A scene the product cannot
  // reach says so and stays tolerated; anything else is the eyes going blind.
  const blind = missing.filter((why) => !why.includes(A_GAP_IN_THE_PRODUCT));
  if (blind.length === 0) return;
  console.log(`${blind.length} of them are not a declared gap: the capture is blind`);
  process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
