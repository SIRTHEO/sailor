/**
 * What only a drawn surface can be measured on.
 *
 * The sheet test reads rules and the contrast test walks a computed DOM;
 * neither knows how wide a column ends up in a real window, nor whether what
 * is in the document sits inside the frame. Runs apart from `npm test`, which
 * stays browserless: `npm run check:canvas`.
 *
 * This does not prove the window is good. It proves it is there.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium, type Browser } from "playwright";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const URL = "http://localhost:5183/";

/** The two capture widths, plus the two thresholds between them. */
const WIDTHS = [375, 760, 1100, 1440];

/**
 * ES5 and no named functions on purpose: the compiler injects a name helper
 * that does not exist inside `evaluate`, and the measure dies there.
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
  // A width is not enough: a floating panel leaves the canvas wide and covers
  // it, so ask what is actually under the pointer.
  var reachable = null;
  if (pane) {
    var hit = document.elementFromPoint(pane.left + pane.width / 2, pane.top + pane.height / 2);
    reachable = hit ? (hit.closest(".panel") ? "panel" : "canvas") : "nothing";
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
    console.log(`--- ${URL} already answers: using it ---`);
  } else {
    console.log("--- starting vite ---");
    vite = spawn("npm", ["run", "dev"], { cwd: root, stdio: "ignore" });
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline && !(await serving())) {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    if (!(await serving())) throw new Error(`vite does not answer on ${URL}`);
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
      // The framing is a declared transition: let it finish before measuring.
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
        `${String(width).padStart(5)}px  rail ${String(m.rail).padStart(3)} · ` +
        `canvas ${String(m.canvas).padStart(4)} · panel ${String(m.panel).padStart(3)}   ` +
        `nodes in view: ${m.inView}/${m.nodes}`;

      // The floor is not zero: a thirty-pixel canvas is a leftover of the
      // layout maths, not something anyone would look at.
      const canvasOk = (m.canvas ?? 0) >= 120;
      // Not "all of them": a flow wider than the screen does not fit, and that
      // is right. None of them means whoever opens it sees nothing.
      const framedOk = m.nodes === 0 || m.inView > 0;
      // Below the narrow threshold the inspector floats over the canvas. One
      // that floats with nothing to say covers what it covers for nothing.
      const reachableOk = !m.panelEmpty || m.reachable !== "panel";

      console.log(`${canvasOk && framedOk && reachableOk ? "  ✓" : "  ✖"} ${line}   at the centre: ${m.reachable}`);
      if (!canvasOk) failures.push(`${width}px: canvas is ${m.canvas}px wide — under 120 it is not a canvas`);
      if (!framedOk) failures.push(`${width}px: ${m.nodes} nodes in the document, none inside the view`);
      if (!reachableOk) failures.push(`${width}px: the panel sits at the centre of the canvas, and it is empty`);

      await context.close();
    }
  } finally {
    await browser?.close();
    vite?.kill();
  }

  if (failures.length > 0) {
    console.error(`\n✖ ${failures.length} failures:`);
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log("\n✓ the canvas is there at every width, and frames what it holds.");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
