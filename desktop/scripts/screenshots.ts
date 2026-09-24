/**
 * The eyes: draws the window, photographs it, and writes down its tree.
 *
 * Contract, set by the `schermo` step of the design flows: an npm script named
 * `screenshots` that starts the project, captures at 375 and 1440 pixels, and
 * saves into the build output. **A PICTURE IS NOT A SOURCE**: it is written
 * under `target/`, which nothing tracks, so a render never lands in the tree.
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
 *
 * With `--walkthrough` (`npm run walkthrough`) the eyes also judge, and refuse:
 * a scene not reached, a page that scrolls sideways, an error the page raised.
 * They judge that each screen renders and fits, never whether it looks right;
 * the pictures stay for whoever wants to look.
 */
import { mkdir, realpath, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser, type ConsoleMessage, type Page } from "playwright";
import { createServer as createVite, type ViteDevServer } from "vite";
import type { Section } from "../src/places";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const outDir = join(root, "..", "target", "screenshots");

const WALKTHROUGH = process.argv.includes("--walkthrough");

let URL = "";

/**
 * The product's own names for its places. They come through vite, not an
 * import: the catalogue under them is a module only vite's plugin answers, and
 * outside it the script stopped loading at all.
 */
let places: typeof import("../src/places");

/** A pixel of rounding is not a page that scrolls sideways. */
const SIDEWAYS_TOLERANCE = 1;

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

/** A product name is data, not a pattern: matched whole, escaped first. */
function wholly(name: string): RegExp {
  return new RegExp(`^\\s*${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`);
}

/**
 * Opens what the palette offers, by the label it draws.
 *
 * The permanent strip of destinations is gone, so a locator that looks for one
 * waits for an element no version of the page renders. What does not hang
 * under a tree is reached by typing, and the capture asks for it that way.
 */
async function openByPalette(page: Page, label: string): Promise<void> {
  const opener = page.locator(".topbar__palette");
  await opener.waitFor({ state: "visible", timeout: 5000 });
  await opener.click();
  const entry = page
    .locator(".palette__entry")
    .filter({ has: page.locator(".palette__label", { hasText: wholly(label) }) })
    .first();
  await entry.waitFor({ state: "visible", timeout: 5000 });
  await entry.click();
  // An entry that lands nowhere would photograph the screen before it, and the
  // capture would call that screen by the name of this one.
  await page.locator(".topbar__crumb", { hasText: wholly(label) }).first().waitFor({
    state: "visible",
    timeout: 5000,
  });
}

/**
 * Opens a place by the name the product gives it, not by a word written here:
 * copied once, the names went stale and the capture went blind in silence.
 * Where a place hangs decides the gesture: under a tree it is a row of the
 * column, and everywhere else only the palette names it.
 */
async function openPlace(page: Page, id: Section): Promise<void> {
  const place = places.placeNamed(id);
  if (!place) throw new Error(`no place «${id}»: the product does not have it`);
  if (!places.UNDER_A_TREE.includes(id)) {
    await openByPalette(page, place.name);
    return;
  }
  const tab = page.getByRole("button", { name: new RegExp(`^\\s*${place.name}`, "i") }).first();
  await tab.waitFor({ state: "visible", timeout: 5000 });
  await tab.click();
}

/** A row of the machine's ground, by its row and not by a label typed here. */
async function openMachineRow(page: Page, id: string): Promise<void> {
  const row = places.MACHINE.find((one) => one.id === id);
  if (!row) throw new Error(`no machine row «${id}»: the product does not have it`);
  await openByPalette(page, row.name);
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
      await openMachineRow(page, "equipment");
    },
  },
  {
    name: "flow-map",
    what: "which flow calls which: the one view that reads the whole machine at once. Outside the native shell there are no files to read, so what this captures is the state that says why — the one a person meets when the engine is not answering",
    reach: async (page) => {
      await openPlace(page, "flowmap");
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

/** A port nobody holds, asked of the system rather than guessed. */
async function aFreePort(): Promise<number> {
  return await new Promise((resolve, reject) => {
    const probe = createServer();
    probe.once("error", reject);
    probe.listen(0, "localhost", () => {
      const address = probe.address();
      probe.close(() =>
        typeof address === "object" && address ? resolve(address.port) : reject(new Error("no port")),
      );
    });
  });
}

/**
 * A console line as the page meant it: React writes its warnings as a format
 * and puts the component that did it among the arguments, which `text()` drops.
 */
async function spelledOut(message: ConsoleMessage): Promise<string> {
  const args = await Promise.all(message.args().map((arg) => arg.jsonValue().catch(() => undefined)));
  if (typeof args[0] !== "string") return message.text();
  const rest = args.slice(1);
  const head = args[0].replace(/%[sdifoOc]/g, () => (rest.length > 0 ? String(rest.shift()) : ""));
  return [head, ...rest.map(String)].join(" ").replace(/\s+/g, " ").slice(0, 400);
}

/** What a scene did wrong once it was reached; empty when it rendered and fits. */
async function whatIsWrong(page: Page, width: number, heard: Promise<string>[]): Promise<string[]> {
  const raised = await Promise.all(heard);
  const wrong = raised.map((said) => `the page raised: ${said}`);
  const wide = (await page.evaluate("document.documentElement.scrollWidth")) as number;
  if (wide > width + SIDEWAYS_TOLERANCE) {
    wrong.push(`scrolls sideways: ${wide}px of page in a ${width}px window`);
  }
  return wrong;
}

async function main(): Promise<void> {
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });

  const missing: string[] = [];
  const refused: string[] = [];

  // Its own server on a free port, never one found answering: that one may
  // draw another tree. `force` goes past the optimizer cache every worktree
  // shares, which otherwise serves another tree's modules without a word.
  const port = await aFreePort();
  const vite: ViteDevServer = await createVite({
    root,
    logLevel: "error",
    // A tree that borrows another's packages through a link is served them
    // by their real path, which the allowed folders do not cover: the fonts
    // came back 403 and every scene was refused for it.
    server: { port, strictPort: true, fs: { allow: [await realpath(join(root, "node_modules"))] } },
    optimizeDeps: { force: true },
  });
  await vite.listen();
  URL = `http://localhost:${port}/`;
  console.log(`--- vite on ${port} ---`);
  places = (await vite.ssrLoadModule("/src/places.ts")) as typeof places;

  let browser: Browser | null = null;
  try {
    // The installed Chrome, not a download: hundreds of megabytes to
    // photograph a few scenes is a price this tree need not pay. A machine
    // without it uses the chromium the driver already holds, and says so.
    browser = await chromium.launch({ channel: "chrome" }).catch(async () => {
      console.log("--- no installed Chrome: the driver's own chromium ---");
      return await chromium.launch();
    });

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
      const raised: Promise<string>[] = [];
      const heard = (said: string) => raised.push(Promise.resolve(said));
      page.on("pageerror", (error) => heard(`uncaught ${error.message}`));
      page.on("console", (message) => {
        if (message.type() !== "error") return;
        // Its address is in the response below; this line would say only the status.
        if (message.text().startsWith("Failed to load resource")) return;
        raised.push(spelledOut(message).then((said) => `console.error ${said}`));
      });
      page.on("response", (response) => {
        if (response.status() >= 400) heard(`${response.status()} ${response.url()}`);
      });
      page.on("requestfailed", (request) => {
        const why = request.failure()?.errorText ?? "";
        // Leaving a page cancels what it still had in flight; nothing failed.
        if (why !== "net::ERR_ABORTED") heard(`request failed ${request.url()}: ${why}`);
      });
      await page.goto(URL, { waitUntil: "networkidle" });

      for (const scene of SCENES) {
        const stem = `${scene.name}-${size.name}`;
        try {
          raised.length = 0;
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

          const wrong = WALKTHROUGH ? await whatIsWrong(page, size.width, raised) : [];
          if (wrong.length === 0) {
            console.log(`  ✓ ${stem}`);
          } else {
            refused.push(`${scene.name} at ${size.width}px: ${wrong.join("; ")}`);
            console.log(`  ✖ ${stem} — ${wrong.join("; ")}`);
          }
        } catch (error) {
          const why = error instanceof Error ? error.message : String(error);
          missing.push(`${stem}: ${why}`);
          if (WALKTHROUGH) refused.push(`${scene.name} at ${size.width}px: not reached: ${why}`);
          console.log(`  ✖ ${stem} — not reached: ${why}`);
        }
      }

      await context.close();
    }
  } finally {
    await browser?.close();
    await vite.close();
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
  if (WALKTHROUGH) {
    if (refused.length === 0) {
      console.log("walkthrough: every scene reached, fits its width and raised nothing");
      return;
    }
    console.log(`walkthrough refused ${refused.length}:\n${refused.map((r) => `- ${r}`).join("\n")}`);
    process.exitCode = 1;
    return;
  }
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
