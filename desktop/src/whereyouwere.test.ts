// @vitest-environment jsdom
/**
 * **A REBUILD IS NOT A REASON TO LOSE YOUR PLACE.** What is written down here
 * is what makes the swap at every build cost nothing.
 */
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { amongThese, rememberWhere, stillThere, STILL_WHERE_YOU_WERE_SECS, whereYouWere } from "./whereyouwere";

beforeEach(() => window.localStorage.clear());
afterEach(() => window.localStorage.clear());

describe("where you were", () => {
  test("NOTHING WRITTEN DOWN IS AN EMPTY ANSWER, not a broken one", () => {
    expect(whereYouWere()).toEqual({});
  });

  test("WHAT IS WRITTEN COMES BACK, and a second note keeps the first", () => {
    rememberWhere({ place: "terminals" });
    rememberWhere({ focus: "prima-corsa" });
    const { at, ...written } = whereYouWere();
    expect(written).toEqual({ place: "terminals", focus: "prima-corsa" });
    expect(at, "a note with no instant cannot be told from one written yesterday").toBeTypeOf("number");
  });

  /**
   * **AN OLD NOTE IS NOT WHERE YOU ARE.** It covers a rebuild and a crash: held
   * for ever, the window would open on the place somebody stood the night
   * before instead of on what waits for them.
   */
  test("AND IT STOPS BEING WHERE YOU WERE ONCE THE NIGHT HAS PASSED", () => {
    const now = 1_000_000;
    expect(stillThere({ place: "terminals", at: now - 30 }, now)).toBe(true);
    expect(stillThere({ place: "terminals", at: now - STILL_WHERE_YOU_WERE_SECS - 1 }, now)).toBe(false);
    expect(stillThere({ place: "terminals" }, now), "a note from before the instant reads as old").toBe(false);
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
