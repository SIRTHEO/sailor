// @vitest-environment jsdom
/**
 * **THE LOOK IS A CHOICE, AND A CHOICE THAT IS FORGOTTEN IS NOT ONE.** Each of
 * the three has to reach the sheet the only way it listens — `data-theme` on
 * the root, or nothing at all — and be there again at the next start.
 */
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { LOOKS, savedLook, wear, wearSavedLook } from "./look";

const root = () => document.documentElement;

beforeEach(() => {
  window.localStorage.clear();
  root().removeAttribute("data-theme");
});

afterEach(() => {
  window.localStorage.clear();
  root().removeAttribute("data-theme");
});

describe("the look this window wears", () => {
  test("WITH NOTHING CHOSEN THE MACHINE ANSWERS, and the sheet is told nothing", () => {
    expect(savedLook()).toBe("the machine's");
    expect(wearSavedLook()).toBe("the machine's");
    // Any value at all takes the answer away from the media query.
    expect(root().hasAttribute("data-theme"), "the sheet was told something").toBe(false);
  });

  test("NIGHT AND DAY REACH THE SHEET AS THE TWO WORDS IT KNOWS", () => {
    wear("night");
    expect(root().getAttribute("data-theme")).toBe("dark");

    wear("day");
    expect(root().getAttribute("data-theme")).toBe("light");
  });

  test("THE WINDOW OPENS ON THE LOOK IT WAS LEFT ON", () => {
    wear("day");

    // A new start: the stamp is gone, only what was written down remains.
    root().removeAttribute("data-theme");
    expect(wearSavedLook()).toBe("day");
    expect(root().getAttribute("data-theme")).toBe("light");
  });

  test("GOING BACK TO THE MACHINE TAKES THE STAMP OFF, it does not stamp a third word", () => {
    wear("night");
    wear("the machine's");
    expect(root().hasAttribute("data-theme"), "the media query cannot answer any more").toBe(false);
    expect(savedLook()).toBe("the machine's");
  });

  test("A WORD NOBODY OFFERS IS NOT WORN", () => {
    // An old name or a half-written value: nothing the sheet answers to.
    window.localStorage.setItem("sailor.look", "midnight");
    expect(savedLook()).toBe("the machine's");
    expect(wearSavedLook()).toBe("the machine's");
    expect(root().hasAttribute("data-theme")).toBe(false);
  });

  test("EVERY LOOK OFFERED IS A LOOK THAT CAN BE WORN", () => {
    // The list the screen draws and the map that stamps are two hands: an
    // answer in one and not the other is a button that does nothing.
    for (const look of LOOKS) {
      wear(look);
      expect(savedLook(), `«${look}» is offered and not remembered`).toBe(look);
    }
  });
});
