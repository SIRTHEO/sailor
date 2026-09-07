// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { StepWhy, renderWhy } from "./StepWhy";
import type { Why } from "./engine";

const asked = (over: Partial<Why> = {}): Why => ({
  because: "condition",
  held: false,
  looked_at: "/report/status",
  wanted: 'equals "failed"',
  found: "passed",
  found_was_there: true,
  ...over,
});

describe("the reason a step did what it did", () => {
  test("SAYS THE PLACE, THE DEMAND AND WHAT WAS THERE, not only the verdict", () => {
    const line = renderWhy(asked());
    expect(line).toContain("/report/status");
    expect(line).toContain('equals "failed"');
    expect(line).toContain('"passed"');
    expect(line).toMatch(/^skipped/);
  });

  test("AND NOTHING THERE READS DIFFERENTLY FROM SOMETHING EMPTY", () => {
    const absent = renderWhy(asked({ found: null, found_was_there: false }));
    const empty = renderWhy(asked({ found: null, found_was_there: true }));
    expect(absent).toContain("nothing was there");
    expect(empty).toContain("null");
    expect(
      absent,
      "a place that does not exist and a place holding null are two different mistakes",
    ).not.toBe(empty);
  });

  test("a condition that reads no place says the demand alone", () => {
    const line = renderWhy(asked({ looked_at: null, wanted: "the input equals 3", found: 4 }));
    expect(line).toBe("skipped: the input equals 3 — it was 4");
  });

  test("and the step that ran says so", () => {
    expect(renderWhy(asked({ held: true, found: "failed" }))).toMatch(/^ran/);
  });

  test("THE READER SEES IT WITHOUT OPENING ANYTHING", () => {
    render(<StepWhy why={asked()} />);
    expect(screen.getByText(/nothing was there|"passed"/)).toBeTruthy();
  });
});
