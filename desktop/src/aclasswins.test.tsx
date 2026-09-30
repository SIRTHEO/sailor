// @vitest-environment jsdom
import stylesheetSource from "./styles.css?raw";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { TheWindow } from "./window/TheWindow";
import { parseStylesheet, type CssRule, type Stylesheet } from "./contrast";

/**
 * **A CLASS ON A BUTTON WINS OVER THE PAINT UNDER IT**, measured on the drawn
 * DOM. No rule of the sheet had to be wrong for this to break: a guard on the
 * bare `button` rule outweighed them all, and thirty of the window's
 * thirty-two buttons were boxes over rules that said `border: none`.
 */

afterEach(cleanup);

class NoResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

let sheet: Stylesheet;

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = NoResizeObserver;
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
  sheet = parseStylesheet(stylesheetSource);
});

/** The four the generic button lays down, and the four a flat row takes back. */
const PAINT = ["border", "border-radius", "padding", "background"];

function matches(element: Element, selector: string): boolean {
  try {
    return element.matches(selector);
  } catch {
    return false;
  }
}

/** Last writer for one property among a set of rules, cascade order. */
function winnerOf(rules: CssRule[], property: string): string | undefined {
  const said = rules
    .filter((rule) => rule.declarations.some(([name]) => name === property))
    .sort((left, right) =>
      left.specificity !== right.specificity
        ? left.specificity - right.specificity
        : left.order - right.order,
    );
  const last = said.at(-1);
  return last?.declarations.filter(([name]) => name === property).at(-1)?.[1];
}

/** Every button drawn, with what its classes ask for and what it is given. */
function overpainted(root: Element): string[] {
  const wrong: string[] = [];
  for (const button of Array.from(root.querySelectorAll("button"))) {
    const hitting = sheet.rules.filter((rule) => matches(button, rule.selector));
    const byItsOwnClass = hitting.filter((rule) => rule.selector.includes("."));
    if (byItsOwnClass.length === 0) continue;
    for (const property of PAINT) {
      const asked = winnerOf(byItsOwnClass, property);
      if (asked === undefined) continue;
      const given = winnerOf(hitting, property);
      if (given === asked) continue;
      wrong.push(`${button.className || button.tagName}: ${property} asked «${asked}», given «${given ?? "nothing"}»`);
    }
  }
  return wrong;
}

describe("the paint under a button is a floor, not a lid", () => {
  test("in the window as it opens", () => {
    const { container } = render(<App />);
    const buttons = container.querySelectorAll("button").length;
    expect(buttons, "no button drawn: this check would guard nothing").toBeGreaterThan(5);
    expect(overpainted(container), "a class on a button, overruled by the paint under it").toEqual([]);
  });

  test("in the three columns", () => {
    const { container } = render(<TheWindow native={false} />);
    expect(container.querySelectorAll(".window-rail__button").length).toBe(5);
    expect(overpainted(container), "a class on a button, overruled by the paint under it").toEqual([]);
  });
});
