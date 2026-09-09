// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { spendWords } from "./Bar";
import type { DaySummary } from "./engine";
import { figureOf, figureUnits, figureWords, MISSING } from "./figure";
import { Today } from "./Now";

/**
 * **WHAT AN INCOMPLETE SUM CAN GET WRONG IN SILENCE.** It draws. It is
 * plausible. It is lower than the truth, and nothing about it says so where
 * the eye lands. The three shapes below are the only three, and the mark for
 * the unmeasured part is inside the number in two of them.
 */

afterEach(cleanup);

const money = (micros: number) => `$${(micros / 1_000_000).toFixed(2)}`;

function day(over: Partial<DaySummary>): DaySummary {
  return {
    ledger_present: true,
    runs: 3,
    went: 3,
    broke: 0,
    still_open: 0,
    input_tokens: 100,
    output_tokens: 20,
    cached_tokens: 0,
    cache_write_tokens: 0,
    cost_micros: 12_400_000,
    unmeasured: 0,
    unpriced: 0,
    tokens_by_model: {},
    ...over,
  };
}

describe("a sum that does not contain everything", () => {
  test("THE MARK TAKES THE PLACE OF THE MISSING PART, on the same line as the digits", () => {
    expect(figureWords(figureOf(12_400_000, 3), money)).toBe(`at least $12.40 + ${MISSING}`);
    expect(figureUnits(figureOf(12_400_000, 3), money)).toBe(`at least $12.40 + ${MISSING}`);
  });

  test("NOTHING PRICED IS NEVER ZERO", () => {
    // `at least $0.00` is true, and reads as a small outlay: the one shape a
    // reader cannot recover from.
    expect(figureOf(0, 4)).toEqual({ shape: "unknown" });
    expect(figureWords(figureOf(0, 4), money)).toBe(`cost ${MISSING}`);
    expect(figureWords(figureOf(0, 4), money)).not.toContain("0");
    expect(figureUnits(figureOf(0, 4), money)).toBe(MISSING);
  });

  test("a covered sum carries no mark: the doubt is a fact, not a habit", () => {
    expect(figureWords(figureOf(12_400_000, 0), money)).toBe("$12.40");
    expect(figureWords(figureOf(0, 0), money)).toBe("$0.00");
  });

  test("the whole expression is one piece of text, at one size", () => {
    render(<Today summary={day({ unpriced: 3 })} />);
    const figure = document.querySelector(".today__cell .figure") as HTMLElement;
    expect(figure.textContent).toBe(`at least $12.400 + ${MISSING}`);
    // THE MUTANT: send the mark to `.today__caveat`. Nothing about the price
    // may sit outside the figure, where it reads as a note under a number.
    expect(document.querySelector(".today__caveat")).toBeNull();
  });

  test("the calls without tokens are still said, and they are not the price", () => {
    render(<Today summary={day({ unmeasured: 7 })} />);
    expect(document.querySelector(".figure")?.textContent).toBe("$12.400");
    expect(screen.getByText(/7 calls without tokens/)).toBeTruthy();
  });

  test("the bar of the window says it in the same shape", () => {
    expect(spendWords(day({ unpriced: 2 }))).toBe(`at least $12.40 + ${MISSING} today`);
    expect(spendWords(day({ cost_micros: 0, unpriced: 2 }))).toBe(`cost ${MISSING} today`);
    expect(spendWords(day({}))).toBe("$12.40 today");
  });
});
