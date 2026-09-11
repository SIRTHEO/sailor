/**
 * THE CAPTURE LOOKS A PLACE UP WHERE EVERY PLACE IS. Six scenes were never
 * photographed because the scripts asked `PLACES` for the board, and the board
 * hangs under a tree: the capture refused with «the product does not have it»
 * about a place the product has. A blind capture reports an absence it caused.
 */
import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { SECTIONS, placeNamed } from "./places";

const scripts = ["../scripts/screenshots.ts", "../scripts/check-canvas.ts"];

describe("the capture can reach every place the product has", () => {
  test("EVERY SECTION IS FINDABLE BY THE ONE LOOKUP", () => {
    for (const id of SECTIONS) {
      expect(placeNamed(id), `no place «${id}»`).not.toBeNull();
    }
  });

  test("A SCRIPT NEVER LOOKS FOR A PLACE IN THE FIXED LIST ALONE", () => {
    for (const path of scripts) {
      const source = readFileSync(new URL(path, import.meta.url), "utf8");
      expect(source, path).not.toMatch(/PLACES\s*\.\s*find/);
    }
  });
});
