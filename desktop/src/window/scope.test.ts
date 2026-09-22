import { describe, expect, test } from "vitest";
import { scopeInForce, scopeName, scopesOffered } from "./scope";

describe("here, and everywhere", () => {
  test("A WORKSPACE NOBODY IS IN OFFERS NO «HERE»", () => {
    expect(scopesOffered(false)).toEqual(["all"]);
    expect(scopesOffered(true)).toEqual(["here", "all"]);
  });

  test("a scope that is not on offer falls back, it does not count nothing", () => {
    expect(scopeInForce("here", false)).toBe("all");
    expect(scopeInForce("here", true)).toBe("here");
    expect(scopeInForce("all", false)).toBe("all");
  });

  test("both sides have a word, and neither is a raw key", () => {
    for (const side of ["here", "all"] as const) {
      expect(scopeName(side)).not.toContain("window.scope.");
    }
  });
});
