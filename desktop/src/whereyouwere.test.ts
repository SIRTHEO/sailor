// @vitest-environment jsdom
/**
 * **A REBUILD IS NOT A REASON TO LOSE YOUR PLACE.** What is written down here
 * is what makes the swap at every build cost nothing.
 */
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { amongThese, rememberWhere, whereYouWere } from "./whereyouwere";

beforeEach(() => window.localStorage.clear());
afterEach(() => window.localStorage.clear());

describe("where you were", () => {
  test("NOTHING WRITTEN DOWN IS AN EMPTY ANSWER, not a broken one", () => {
    expect(whereYouWere()).toEqual({});
  });

  test("WHAT IS WRITTEN COMES BACK, and a second note keeps the first", () => {
    rememberWhere({ place: "terminals" });
    rememberWhere({ focus: "prima-corsa" });
    expect(whereYouWere()).toEqual({ place: "terminals", focus: "prima-corsa" });
  });

  test("THE BENCH IS WRITTEN DOWN TOO: the terminal survives the swap, and so does what it is for", () => {
    const bench = { terminalId: "t-9", runId: "prima-corsa", stepId: "review", mandate: "read the diff" };
    rememberWhere({ place: "terminals", bench });
    expect(whereYouWere().bench).toEqual(bench);
    // Closing the step clears it: a bench back after a verdict asks twice.
    rememberWhere({ bench: null });
    expect(whereYouWere().bench).toBeNull();
  });

  test("A HALF-WRITTEN NOTE IS DROPPED WHOLE", () => {
    // Storage is shared with everything else in this window and survives every
    // version of it: what comes back is not necessarily what was written.
    window.localStorage.setItem("sailor.where", "{ not json");
    expect(whereYouWere()).toEqual({});
    window.localStorage.setItem("sailor.where", '"terminals"');
    expect(whereYouWere(), "a string was read as a place").toEqual({});
    window.localStorage.setItem("sailor.where", "[1,2]");
    expect(whereYouWere(), "a list was read as a place").toEqual({});
  });
});

describe("a name the window no longer has", () => {
  const PLACES = ["board", "terminals", "ledger"] as const;

  test("IS NOT A PLACE: it opens where everybody starts", () => {
    expect(amongThese("changes", PLACES, "board")).toBe("board");
    expect(amongThese(undefined, PLACES, "board")).toBe("board");
  });

  test("and one the window does have is honoured", () => {
    expect(amongThese("terminals", PLACES, "board")).toBe("terminals");
  });
});
