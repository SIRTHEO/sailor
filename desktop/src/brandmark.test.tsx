// @vitest-environment jsdom
/**
 * **THE POINT IS THAT NOTHING HAS TO BE COMPLETE**, so what is checked is the
 * two rules that make incompleteness harmless — a slug nobody has a mark for,
 * and a mark nobody can see — and that the build carries only what was asked.
 */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { BrandMark } from "./BrandMark";
import { readFileSync, readdirSync } from "node:fs";
import { GROUND, MARKS, contrast, hueOfSlug, legible, monogramOf, rgbOf } from "./brands";

describe("the marks the build carries", () => {
  test("holds the brands the descriptors declare and not the catalogue", () => {
    // 3461 brands exist: this number rising is the plugin having stopped.
    expect(Object.keys(MARKS).length).toBeLessThan(60);
    expect(MARKS["claudecode"]?.hex).toBe("#d97757");
    expect(MARKS["claudecode"]?.path.length).toBeGreaterThan(20);
  });

  test("has no mark for the brands their owners withdrew", () => {
    // Withdrawn over trademarks. Sailor declares both slugs anyway: the brand
    // is the truth, the missing drawing is somebody else's.
    expect(MARKS["openai"]).toBeUndefined();
    expect(MARKS["aws"]).toBeUndefined();
  });
});

describe("the marks kept by hand", () => {
  // Read from the project's root, not from `import.meta.url`: under jsdom that
  // URL is an http one, and `node:fs` refuses it.
  const folder = "src/brands";
  const kept = readdirSync(folder).filter((name) => name.endsWith(".svg"));

  test("holds only brands the catalogue is missing, so it empties itself", () => {
    const upstream = new Set(
      (JSON.parse(
        readFileSync("node_modules/simple-icons/data/simple-icons.json", "utf8"),
      ) as { title: string; slug?: string }[]).map(
        (icon) => icon.slug ?? icon.title.toLowerCase().replace(/[^a-z0-9]/g, ""),
      ),
    );
    for (const name of kept) {
      const slug = name.replace(/\.svg$/, "");
      expect(upstream.has(slug), `${slug} is in simple-icons now: delete this copy`).toBe(false);
    }
  });

  test("gives each mark the three things the build refuses it without", () => {
    for (const name of kept) {
      const drawing = readFileSync(`${folder}/${name}`, "utf8");
      expect(/<svg[^>]*\sviewBox="0 0 24 24"/.test(drawing), `${name}: a 24×24 viewBox`).toBe(true);
      expect(/<svg[^>]*\sfill="#[0-9a-fA-F]{6}"/.test(drawing), `${name}: a fill of six hex digits`).toBe(true);
      expect(/<title>[^<]+<\/title>/.test(drawing), `${name}: a title`).toBe(true);
      expect(/\sd="[^"]+"/.test(drawing), `${name}: one path`).toBe(true);
    }
  });
});

describe("a colour that can be seen on the ground", () => {
  test("leaves a brand alone when its own colour already clears the floor", () => {
    expect(legible("#d97757")).toBe("#d97757");
    expect(legible("#8e75b2")).toBe("#8e75b2");
  });

  test("lifts a colour that cannot be seen, and keeps its hue", () => {
    for (const black of ["#000000", "#191919", "#181717"]) {
      const lifted = legible(black);
      expect(contrast(black, GROUND)).toBeLessThan(3);
      expect(contrast(lifted, GROUND)).toBeGreaterThanOrEqual(3);
    }
    const rust = legible("#000000");
    expect(rust.slice(1, 3)).toBe(rust.slice(3, 5));
  });

  test("gives every unnamed brand a colour that can be seen, and always the same one", () => {
    const slugs = ["openai", "aws", "jq", "ripgrep", "googleantigravity", "socraticode", "orca"];
    for (const slug of slugs) {
      expect(hueOfSlug(slug)).toBe(hueOfSlug(slug));
      expect(contrast(hueOfSlug(slug), GROUND)).toBeGreaterThanOrEqual(3);
      expect(contrast(hueOfSlug(slug), "#19171d")).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("the monogram", () => {
  test("gives the brands with no mark hues far enough apart to be told apart", () => {
    // A modulo inside the hash left `openai` and `aws` 8° apart, which on two
    // squares side by side is one colour. The five below are the whole of what
    // Sailor declares and cannot draw.
    const hues = ["openai", "aws", "jq", "ripgrep", "googleantigravity"].map((slug) => {
      const { red, green, blue } = rgbOf(hueOfSlug(slug));
      const high = Math.max(red, green, blue);
      const span = high - Math.min(red, green, blue);
      const turn =
        high === red ? ((green - blue) / span + (green < blue ? 6 : 0))
        : high === green ? (blue - red) / span + 2
        : (red - green) / span + 4;
      return turn * 60;
    });
    for (const one of hues) {
      for (const other of hues) {
        if (one === other) continue;
        const apart = Math.abs(one - other);
        expect(Math.min(apart, 360 - apart)).toBeGreaterThan(15);
      }
    }
  });


  test("takes the first letter a name can spare", () => {
    expect(monogramOf("Codex")).toBe("C");
    expect(monogramOf(".ENV")).toBe("E");
    expect(monogramOf("  agy")).toBe("A");
    expect(monogramOf("")).toBe("?");
  });
});

describe("the component", () => {
  afterEach(cleanup);

  test("draws the brand's own mark where there is one", () => {
    render(<BrandMark slug="claudecode" label="Claude Code" />);
    const mark = screen.getByRole("img", { name: "Claude Code" });
    expect(mark.tagName.toLowerCase()).toBe("svg");
    expect(mark.getAttribute("fill")).toBe("#d97757");
  });

  test("draws a monogram where there is none, rather than a hole", () => {
    render(<BrandMark slug="openai" label="Codex" />);
    const mark = screen.getByRole("img", { name: "Codex" });
    expect(mark.textContent).toBe("C");
  });

  test("draws something even for a descriptor that declares no brand at all", () => {
    render(<BrandMark slug="" label="Orca" />);
    const mark = screen.getByRole("img", { name: "Orca" });
    expect(mark.textContent).toBe("O");
    expect(mark.getAttribute("data-brand")).toBe("unnamed");
  });
});
