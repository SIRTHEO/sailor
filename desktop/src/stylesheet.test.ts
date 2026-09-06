import { describe, expect, test } from "vitest";
import stylesheetSource from "./styles.css?raw";
import { isRoleGround, parseColor, parseStylesheet, resolveVars } from "./contrast";

/**
 * **THE PROHIBITIONS AT THE TOP OF `styles.css`, INTERROGATED.** A prohibition
 * without a check does not hold even for the person writing it, in the turn
 * they write it. Whatever can be read off the sheet is read here; rule 6, the
 * contrast pairs, is measured on the painted DOM in `contrast.test.tsx`.
 */

const sheet = parseStylesheet(stylesheetSource);

/** The sheet's rules minus the grounds where the roles are defined. */
const outsideRoot = sheet.rules.filter((rule) => !isRoleGround(rule.selector));

function declarationsOf(property: string): Array<{ selector: string; value: string }> {
  const found: Array<{ selector: string; value: string }> = [];
  for (const rule of outsideRoot) {
    for (const [name, value] of rule.declarations) {
      if (name === property) found.push({ selector: rule.selector, value: value.trim() });
    }
  }
  return found;
}

describe("prohibition 1 — three type families, and only those", () => {
  test("no font stack written by hand outside the roles", () => {
    // A `system-ui` stack on a tool's monogram, and two monospace stacks copied
    // into the console and the code boxes.
    const wrong = declarationsOf("font-family").filter(
      ({ value }) => !/^var\(--font-(display|prose|data)\)$/.test(value),
    );
    expect(wrong).toEqual([]);
  });
});

describe("prohibition 2 — two radii and a pill", () => {
  test("no radius written by hand", () => {
    // `border-radius: 2px` on a lane's mark, `50%` on a trigger's. The rule
    // admits three values, and they are three roles.
    const allowed = /^var\(--radius(-lg|-pill)?\)$/;
    const wrong = declarationsOf("border-radius").filter(({ value }) => !allowed.test(value));
    expect(wrong).toEqual([]);
  });
});

describe("prohibition 3 — one shadow only", () => {
  test("the only shadow is `--shadow`; an inner ring is not a shadow", () => {
    // `inset` floats nothing: it is the focus hairline drawn inside the border,
    // and the rule is about what sits above the canvas.
    const wrong = declarationsOf("box-shadow").filter(
      ({ value }) => value !== "var(--shadow)" && value !== "none" && !value.startsWith("inset "),
    );
    expect(wrong).toEqual([]);
  });
});

describe("prohibition 8 — the sizes stay in the scale", () => {
  test("no size written by hand", () => {
    const wrong = declarationsOf("font-size").filter(
      ({ value }) => !/^var\(--text-[a-z]+\)$/.test(value),
    );
    expect(wrong).toEqual([]);
  });

  /** The roles of `:root`, so a `var()` can be followed to a number. */
  function rootRoles(): Map<string, string> {
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    expect(root, "no `:root`: every check below would follow nothing").toBeDefined();
    return new Map((root as { declarations: Array<[string, string]> }).declarations);
  }

  /** Under 12px a label is recognised by its shape and never read. Followed
   *  through `var()`, so renaming a role below the floor is still red. */
  const FLOOR = 12;

  test("THE SCALE ITSELF HAS NOTHING UNDER THE FLOOR", () => {
    const roles = rootRoles();
    const scale = [...roles].filter(([name]) => /^--text-/.test(name));
    expect(scale.length, "no size roles at all: this test would guard nothing").toBeGreaterThan(3);
    const under = scale
      .map(([name, value]) => [name, Number.parseFloat(resolveVars(value, roles))] as const)
      .filter(([, pixels]) => Number.isFinite(pixels) && pixels < FLOOR);
    expect(under, `a size role under ${FLOOR}px`).toEqual([]);
  });

  test("AND NO RULE IN THE SHEET DECLARES A SIZE UNDER THE FLOOR", () => {
    const roles = rootRoles();
    const declared = declarationsOf("font-size");
    expect(declared.length, "no rule sets a size: this test would guard nothing").toBeGreaterThan(50);
    const under = declared
      .map(({ selector, value }) => ({
        selector,
        pixels: Number.parseFloat(resolveVars(value, roles)),
      }))
      .filter(({ pixels }) => Number.isFinite(pixels) && pixels < FLOOR);
    expect(under, `a rule painting text under ${FLOOR}px`).toEqual([]);
  });

  /** THIS SHEET IS UNLAYERED AND TAILWIND'S UTILITIES ARE NOT, so unlayered
   *  wins whatever the specificity: a rule on bare `button` repaints a shadcn
   *  button and the component becomes a dependency paid for and inert. */
  test("THE BARE `button` RULE STEPS ASIDE FOR A COMPONENT THAT PAINTS ITSELF", () => {
    const onEveryButton = sheet.rules
      .map((rule) => rule.selector.trim())
      .filter((selector) =>
        selector
          .split(",")
          .some((part) => /^button(:[a-z-]+(\([^)]*\))?)*$/.test(part.trim())),
      );
    expect(onEveryButton.length, "nothing paints bare buttons: this guards nothing")
      .toBeGreaterThan(0);
    const unguarded = onEveryButton.filter((selector) => !selector.includes("[data-slot]"));
    expect(unguarded, "a rule that repaints a component styling itself").toEqual([]);
  });

  /** The three weights, and nothing between or beyond them. */
  test("three weights, and only those", () => {
    const allowed = new Set(["400", "500", "600", "normal", "inherit"]);
    const wrong = declarationsOf("font-weight").filter(({ value }) => !allowed.has(value));
    expect(wrong, "a weight outside 400 / 500 / 600").toEqual([]);
  });
});

/** A token defined only inside `@media (prefers-color-scheme: …)` is unset for
 *  whoever asked for neither, and an unset property paints nothing — invisible
 *  in `jsdom`, which skips at-rule bodies. */
describe("the token system", () => {
  /** The thirteen the whole sheet is built out of. */
  const TOKENS = [
    "--surface-0",
    "--surface-1",
    "--surface-2",
    "--border",
    "--text-1",
    "--text-2",
    "--text-3",
    "--accent",
    "--running",
    "--needs-human",
    "--finished",
    "--failed",
    "--spend-warning",
  ];

  test("EVERY TOKEN IS A COLOUR ON BARE `:root`, not only under a media query", () => {
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    const declared = new Map((root as { declarations: Array<[string, string]> }).declarations);
    const missing = TOKENS.filter((name) => parseColor(declared.get(name) ?? "") === null);
    expect(missing, "a token that bare `:root` does not define as a colour").toEqual([]);
  });

  test("and the other scheme gives every one of them a colour of its own", () => {
    const other = new Map(sheet.otherRoot ?? []);
    expect(other.size, "there is no other scheme any more").toBeGreaterThan(0);
    const missing = TOKENS.filter((name) => parseColor(other.get(name) ?? "") === null);
    expect(missing, "a token the other scheme leaves at the first scheme's value").toEqual([]);
  });

  /** Under this window sits a terminal where green already means «it worked»:
   *  a green running step tells the eye the opposite of the truth. */
  test("RUNNING IS BLUE AND FINISHED IS GREEN, under both schemes", () => {
    function hue(text: string): number {
      const color = parseColor(text);
      expect(color, `${text} is not a colour`).not.toBeNull();
      const { r, g, b } = color as { r: number; g: number; b: number };
      const [max, min] = [Math.max(r, g, b), Math.min(r, g, b)];
      if (max === min) return -1;
      const span = max - min;
      const raw =
        max === r ? (g - b) / span : max === g ? 2 + (b - r) / span : 4 + (r - g) / span;
      return ((raw * 60) % 360 + 360) % 360;
    }

    const root = new Map(
      (sheet.rules.find((rule) => rule.selector === ":root") as {
        declarations: Array<[string, string]>;
      }).declarations,
    );
    const other = new Map(sheet.otherRoot ?? []);
    for (const [scheme, roles] of [["night", root], ["day", other]] as const) {
      const running = hue(roles.get("--running") ?? "");
      const finished = hue(roles.get("--finished") ?? "");
      expect(running, `${scheme}: running is not blue`).toBeGreaterThan(180);
      expect(running, `${scheme}: running is not blue`).toBeLessThan(260);
      expect(finished, `${scheme}: finished is not green`).toBeGreaterThan(90);
      expect(finished, `${scheme}: finished is not green`).toBeLessThan(180);
    }
  });
});

describe("every colour goes through a role", () => {
  test("OUTSIDE `:root` THERE IS NO LITERAL COLOUR", () => {
    // No literal colour outside `:root`. It is the precondition for a dark
    // theme: with every tint coming from a role, the second theme is blocked
    // only by the measurements it still needs.
    const literal =
      /#[0-9a-fA-F]{3,8}\b|\brgba?\([^)]*\)|\bhsla?\([^)]*\)|\b(?:white|black|red|blue|green|gray|grey|orange|purple)\b/;
    const wrong: Array<{ selector: string; property: string; value: string }> = [];
    for (const rule of outsideRoot) {
      for (const [property, value] of rule.declarations) {
        if (property === "white-space") continue;
        if (literal.test(value)) wrong.push({ selector: rule.selector, property, value });
      }
    }
    expect(wrong).toEqual([]);
  });

  test("every `:root` role that names a colour is a colour that reads", () => {
    // A misspelled role does not fail: it resolves to nothing and the element
    // inherits its container's colour. Nobody notices.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    expect(root).toBeDefined();
    const tokens = (root as { declarations: Array<[string, string]> }).declarations.filter(
      ([property]) => /^--(bg|paper|raised|rail|line|band-fill|ink|muted|faint|state-|ok|warn|danger|focus|ink-surface|on-ink|optional|lane-)/.test(property),
    );
    expect(tokens.length).toBeGreaterThan(15);
    // A role may stand for another: the names a component asks for are
    // aliases. Resolved before reading, so a misspelt one is still red.
    const roles = new Map((root as { declarations: Array<[string, string]> }).declarations);
    const broken = tokens.filter(([, value]) => parseColor(resolveVars(value, roles)) === null);
    expect(broken).toEqual([]);
  });
});

describe("the sheet is read whole", () => {
  test("WHAT ARRIVES HERE IS THE SHEET AS WRITTEN, not a worked copy", () => {
    // `vitest` returns an empty string for every CSS import until told
    // `css: true`: without this line every check in this file would be green
    // for having read nothing.
    expect(stylesheetSource).toContain("WHAT THIS DIRECTION FORBIDS");
    expect(stylesheetSource.length).toBeGreaterThan(40000);
    expect(sheet.rules.length).toBeGreaterThan(200);
  });

  test("no colour inside an @-rule", () => {
    expect(sheet.colorsInsideAtRules).toBe(0);
  });

  test("THE DARK SCHEME REDEFINES EVERY ROLE THAT CARRIES A COLOUR, and no other", () => {
    // A role left out keeps its light value under the dark ground: legible in
    // the test that never looked, unreadable on the screen. So the two sets of
    // colour roles must be the same set, and every dark value must be a colour.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    // Raw, not resolved: an alias follows its target under the dark ground.
    const lightColours = (root?.declarations ?? []).filter(([, value]) => parseColor(value) !== null).map(([name]) => name);
    const dark = sheet.otherRoot ?? [];
    const darkNames = dark.map(([name]) => name);
    expect(lightColours.length).toBeGreaterThan(30);
    expect(lightColours.filter((name) => !darkNames.includes(name))).toEqual([]);
    expect(darkNames.filter((name) => !lightColours.includes(name) && !/^--(shadow|scrim)$/.test(name))).toEqual([]);
    expect(dark.filter(([name, value]) => parseColor(value) === null && !/^--(shadow|scrim)$/.test(name))).toEqual([]);
  });

  test("THE DAY A PERSON PINS IS THE DAY THE MACHINE WOULD HAVE GIVEN", () => {
    // CSS cannot say «that block, under this selector too», so the day palette
    // is written twice: once for `prefers-color-scheme: light`, once for
    // `[data-theme="light"]`. Two hands drifting apart would give a person who
    // chose day a different day from everyone else's, and every measurement in
    // `contrast.test.tsx` is made against one of the two.
    const pinned = sheet.rules.find((rule) => rule.selector === ':root[data-theme="light"]');
    expect(pinned, "nobody can pin the day any more").toBeDefined();
    expect(sheet.otherRoot, "the machine's own day is gone").not.toBeNull();
    expect(pinned?.declarations).toEqual(sheet.otherRoot);
  });

  test("prohibition 7 is written into the sheet, not only into the comment", () => {
    // `--faint` was abolished by making it identical to `--muted`. If somebody
    // lightens it again, this line says so before the contrast check does.
    const root = sheet.rules.find((rule) => rule.selector === ":root");
    const declarations = new Map((root as { declarations: Array<[string, string]> }).declarations);
    expect(declarations.get("--faint")).toBe(declarations.get("--muted"));
  });
});

/**
 * **PROHIBITION 11 — NO FIXED COLUMN WITHOUT A WAY OUT.** A pixel `width` with
 * `flex-shrink: 0` holds whatever the window does; two of them beside an
 * elastic canvas divide the window before the canvas has a say, and at 375px
 * the rail and the inspector came to 520 while the canvas was zero pixels wide.
 * `npm run check:canvas` measures the drawn geometry; this one reads the cause
 * out of the sheet, in milliseconds and without a browser.
 */
describe("prohibition 11 — a fixed column says how it behaves when narrow", () => {
  /** The narrowest window this project declares it supports: the width at
   *  which `scripts/screenshots.ts` captures, that is, the one somebody has
   *  already decided the window is to be looked at. */
  const NARROWEST = 375;

  /**
   * The rigid COLUMNS: fixed width in pixels, `flex-shrink: 0`, **and a scroll
   * of their own**.
   *
   * The last condition is not a detail, it is what separates a column from a
   * dot. Without it this test also caught `.focusbar__dot` (9px),
   * `.flow-band__mark` (10px) and `.trigger-node__mark` (7px): graphic marks
   * rigid on purpose that lay out nothing. A check that counted them would have
   * had @-rules written onto dots — it would have changed the world over a
   * wrong number.
   *
   * An element that scrolls by itself **contains** something: it is a column.
   * A structural criterion, not a pixel threshold picked by eye.
   */
  const rigid = outsideRoot
    .map((rule) => {
      const declarations = new Map(rule.declarations);
      const width = declarations.get("width")?.trim();
      const shrink = declarations.get("flex-shrink")?.trim();
      const scrolls = declarations.get("overflow-y")?.trim();
      const pixels = width?.match(/^(\d+)px$/);
      if (!pixels || shrink !== "0") return null;
      if (scrolls !== "auto" && scrolls !== "scroll") return null;
      return { selector: rule.selector, width: Number(pixels[1]) };
    })
    .filter((found): found is { selector: string; width: number } => found !== null);

  test("the test looks at the columns, not at the dots", () => {
    // If one day this list empties, the test below turns green for having
    // looked at nothing — and that is how a check dies in silence.
    expect(rigid.length).toBeGreaterThan(0);
    expect(rigid.every((column) => column.width >= 100)).toBe(true);
  });

  test("the rigid columns do not eat the narrowest window on their own", () => {
    const total = rigid.reduce((sum, column) => sum + column.width, 0);
    if (total < NARROWEST) return; // they fit: no way out is needed

    // They do not fit. Then each must appear inside an @-rule that changes its
    // width or takes it out of the way: without one, what sits between them is
    // crushed to zero and nobody sees it.
    const atRules = stylesheetSource.match(/@media[^{]+\{(?:[^{}]|\{[^{}]*\})*\}/g) ?? [];
    const inside = atRules.join("\n");
    const unguarded = rigid.filter(({ selector }) => !inside.includes(selector));

    expect({
      total: `${total}px of rigid columns against a window of ${NARROWEST}px`,
      withNoWayOut: unguarded.map((column) => `${column.selector} (${column.width}px)`),
    }).toEqual({
      total: `${total}px of rigid columns against a window of ${NARROWEST}px`,
      withNoWayOut: [],
    });
  });
});

/**
 * PROHIBITION 9, which had nothing asking after it. Writing the corner marks I
 * put a gradient in this sheet — two identical stops, a solid rectangle spelt
 * as a gradient — and the whole battery stayed green. Put that line back and
 * this goes red, naming the rule it is in.
 */
describe("prohibition 9 — no gradients, frosted glass or blur", () => {
  test("no rule in the sheet declares one", () => {
    const forbidden = /(linear|radial|conic)-gradient|backdrop-filter|blur\s*\(/i;
    const guilty = sheet.rules
      .flatMap((rule) => rule.declarations.map(([property, value]) => ({ rule, property, value })))
      .filter(({ value }) => forbidden.test(value))
      .map(({ rule, property, value }) => `${rule.selector} { ${property}: ${value} }`);
    expect(guilty, "a gradient got into the sheet").toEqual([]);
  });
});

/**
 * ZOOMING IN MUST NOT SHOW LESS. The far node wrapped the name and read it
 * whole; the near node clipped it — so approaching a node lost information,
 * which is the opposite of what the gesture asks. It wraps at every zoom now.
 */
describe("the name of a step reads the same at every zoom", () => {
  function wrapping(selector: string): Map<string, string> {
    const found = new Map<string, string>();
    for (const rule of sheet.rules) {
      if (rule.selector.trim() !== selector) continue;
      for (const [property, value] of rule.declarations) {
        if (["white-space", "overflow-wrap", "-webkit-line-clamp"].includes(property)) {
          found.set(property, value.trim());
        }
      }
    }
    return found;
  }

  test("the far node declares no wrapping of its own", () => {
    const far = wrapping(".step-node[data-far] .step-node__id");
    expect([...far.keys()], "the two zooms disagree about how the name wraps").toEqual([]);
  });

  /* Three and not two: measured in a browser, two lines clip the longest step
     name that exists, and four stops discriminating — a control string of 80
     characters comes back unclipped, which means the measure has gone blind. */
  test("the name wraps, and stops at three lines", () => {
    const near = wrapping(".step-node__id");
    expect(near.get("white-space")).toBe("normal");
    expect(near.get("-webkit-line-clamp")).toBe("3");
  });
});

/**
 * **A SIGNAL LIVES IN A BORDER, NOT IN A PANEL.** The band declared
 * `width: clamp(280px, …)` and pushed the terminal out below 600 pixels, past
 * 454 green tests: prohibition 11 wants three conditions it did not have. The
 * question here is narrower — no signal declares a width at all.
 */
describe("the three signals take no room from the terminal", () => {
  const SIGNALS = [".pane__where", ".pane__tree", ".pane__notch", ".pane__progress"];
  const WIDTHS = ["width", "min-width", "flex-basis"];

  /** The rules that speak of one of the three signals, or of their highlight. */
  function ofSignals(alsoStirred = false): Array<{ selector: string; declarations: Array<[string, string]> }> {
    return outsideRoot.filter((rule) => {
      const selector = rule.selector.trim();
      if (alsoStirred && selector.includes("[data-stirred]")) return true;
      return SIGNALS.some((signal) => selector.startsWith(signal));
    });
  }

  test("the test looks at signals that really exist", () => {
    // Rename a class and the tests below go green for having looked at
    // nothing: that is how a check dies in silence.
    const seen = SIGNALS.filter((signal) =>
      sheet.rules.some((rule) => rule.selector.trim().startsWith(signal)),
    );
    expect(seen).toEqual(SIGNALS);
    expect(ofSignals(true).length).toBeGreaterThan(SIGNALS.length);
  });

  /** `min-width: 0` is no floor: it is the valve that lets a thing shrink. */
  const NO_FLOOR = new Set(["0", "0px", "auto", "none"]);

  test("NO SIGNAL DECLARES A WIDTH, in any unit", () => {
    const guilty = ofSignals().flatMap((rule) =>
      rule.declarations
        .filter(([property, value]) => WIDTHS.includes(property) && !NO_FLOOR.has(value.trim()))
        .map(([property, value]) => `${rule.selector.trim()} { ${property}: ${value.trim()} }`),
    );
    expect(guilty, "a signal with a width is a column, and it takes it from the terminal").toEqual([]);
  });

  test("THE NOTCH IS OUT OF THE FLOW: it sits on the border, not in line in the header", () => {
    const notch = new Map(
      sheet.rules
        .filter((rule) => rule.selector.trim() === ".pane__notch")
        .flatMap((rule) => rule.declarations),
    );
    expect(notch.get("position")?.trim()).toBe("absolute");
  });

  /**
   * **NOTHING MOVES WHEN NOTHING HAPPENED.** A mark that pulses makes a silent
   * agent look alive, and a crossing is a colour that stops, not a motion.
   */
  test("NO SIGNAL AND NO HIGHLIGHT DECLARES AN ANIMATION", () => {
    const moving = ["animation", "animation-name", "transition", "transform"];
    const guilty = ofSignals(true).flatMap((rule) =>
      rule.declarations
        .filter(([property]) => moving.includes(property))
        .map(([property, value]) => `${rule.selector.trim()} { ${property}: ${value.trim()} }`),
    );
    expect(guilty, "a signal that moves on its own").toEqual([]);
  });
});

/**
 * **NARROW, THE WORK KEEPS THE ROOM.** Three boxes each claiming a share of the
 * viewport left the graph 99 pixels: what helps you choose work is bounded in
 * pixels there, what shows the work happening keeps the rest.
 */
describe("the narrow window budgets its height", () => {
  /** The body of the `max-width: 760px` block, read off the raw sheet. */
  function narrowBlock(): string {
    const at = stylesheetSource.indexOf("@media (max-width: 760px)");
    expect(at, "no narrow block in the sheet: this test would guard nothing").toBeGreaterThan(-1);
    let depth = 0;
    for (let i = stylesheetSource.indexOf("{", at); i < stylesheetSource.length; i++) {
      if (stylesheetSource[i] === "{") depth++;
      if (stylesheetSource[i] === "}" && --depth === 0) return stylesheetSource.slice(at, i);
    }
    throw new Error("the narrow block never closes");
  }

  test("THE COLUMN THAT HELPS YOU CHOOSE IS BOUNDED IN PIXELS, NOT IN VIEWPORTS", () => {
    const world = narrowBlock().match(/\.world\s*\{([^}]*)\}/);
    expect(world, "no `.world` rule in the narrow block").not.toBeNull();
    const cap = world![1].match(/max-height:\s*([\d.]+)(px|vh|dvh|%)/);
    expect(cap, "`.world` claims no height at all when narrow").not.toBeNull();
    expect(cap![2], "a share of the viewport shrinks the work as fast as the window").toBe("px");
    expect(Number(cap![1])).toBeLessThanOrEqual(64);
  });

  test("AND THE GRAPH CAN GIVE GROUND, or the band pushes it out of its box", () => {
    expect(narrowBlock()).toMatch(/\.canvas\s*\{[^}]*min-height:\s*0/);
  });
});
