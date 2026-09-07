import stylesheet from "./styles.css?raw";
import { describe, expect, test } from "vitest";

/**
 * **A MESSAGE THAT DOES NOT SAY WHAT IT SAYS.** The note holds «7 steps» and is
 * clipped to 74 pixels for it; a broken flow puts its reason there.
 */

/** The declarations of one rule, by its selector, as the stylesheet writes them. */
function ruleFor(selector: string): string {
  const at = stylesheet.indexOf(`${selector} {`);
  expect(at, `no rule declares «${selector}»`).toBeGreaterThan(-1);
  return stylesheet.slice(at, stylesheet.indexOf("}", at));
}

describe("the reason a flow is broken", () => {
  test("IS NOT CLIPPED THE WAY A STEP COUNT IS", () => {
    const broken = ruleFor(".rail__item[data-broken] .rail__note");
    expect(broken, "the reason keeps the width meant for «7 steps»").toContain("max-width: none");
    expect(broken, "a sentence on one line with no room is an ellipsis").toContain(
      "white-space: normal",
    );
    // The control: the ordinary note is still the clipped one, or this rule
    // undoes nothing and the test guards a stylesheet that clips nowhere.
    const ordinary = ruleFor(".rail__note");
    expect(ordinary, "the ordinary note is no longer the clipped one").toContain("text-overflow");
    expect(ordinary).toContain("max-width");
  });
});
