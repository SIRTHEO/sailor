import { describe, expect, test } from "vitest";
import screenshotsSource from "../../scripts/screenshots.ts?raw";
import tauri from "../../src-tauri/tauri.conf.json";

/**
 * **TWO CONTRACTS THAT DISAGREE MAKE A PICTURE OF NOTHING.** The window opens
 * no narrower than `minWidth`, but the script photographed it at 375px, where
 * a reviewer read the starved field as a defect of the product, not the shot.
 */

/** The widths the script declares, by the name a scene asks for them by. */
function widthsByName(): Map<string, number> {
  const list = /const WIDTHS = \[([^\]]*)\]/s.exec(screenshotsSource);
  if (!list) throw new Error("the capture script no longer declares WIDTHS");
  const found = new Map<string, number>();
  for (const [, name, width] of list[1].matchAll(/name: "([^"]+)", width: (\d+)/g)) {
    found.set(name, Number(width));
  }
  return found;
}

/** What one scene is captured at: its own `at`, or the contract's default. */
function capturedAt(scene: string): string[] {
  const block = new RegExp(`name: "${scene}"[\\s\\S]*?\\n  \\},`).exec(screenshotsSource);
  if (!block) throw new Error(`the capture script has no scene «${scene}»`);
  const own = /\bat: \[([^\]]*)\]/.exec(block[0]);
  if (!own) {
    const fallback = /const THE_CONTRACT = \[([^\]]*)\]/.exec(screenshotsSource);
    if (!fallback) throw new Error("no default widths are declared");
    return [...fallback[1].matchAll(/"([^"]+)"/g)].map(([, name]) => name);
  }
  return [...own[1].matchAll(/"([^"]+)"/g)].map(([, name]) => name);
}

describe("the window is photographed at widths it can have", () => {
  test("NO SCENE OF THE WINDOW IS CAPTURED BELOW minWidth", () => {
    const floor = tauri.app.windows[0].minWidth;
    const widths = widthsByName();
    const asked = capturedAt("the-window").map((name) => {
      const found = widths.get(name);
      if (found === undefined) throw new Error(`the scene asks for a width «${name}» nobody declares`);
      return found;
    });
    expect(asked.length, "a scene captured at no width is captured nowhere").toBeGreaterThan(0);
    expect(asked.filter((width) => width < floor)).toEqual([]);
  });

  test("and the screens keep the two widths of the contract", () => {
    expect(capturedAt("now")).toEqual(["375", "1440"]);
  });
});
