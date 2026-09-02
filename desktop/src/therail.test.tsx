// @vitest-environment jsdom
/**
 * **THE COLUMN GROUPS FLOWS BY WHERE THEY COME FROM.** Three sources reach the
 * window, and flat they were indistinguishable. On a name clash the most
 * specific wins, so the group also answers which of two namesakes runs.
 */
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";

afterEach(cleanup);

// React Flow measures its own box on mount, and jsdom has nobody to do it:
// without these two the canvas never mounts and the column is never drawn.
beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

/** The window opens elsewhere: the board sits behind a place to be chosen. */
function goToFlows(): void {
  screen.getByRole("button", { name: /^Flussi/ }).click();
}

describe("the column, grouped by origin", () => {
  test("EVERY FLOW SITS UNDER THE SOURCE IT CAME FROM", () => {
    const { container } = render(<App />);
    goToFlows();

    const groups = [...container.querySelectorAll(".rail__origin")].map(
      (heading) => heading.textContent?.trim() ?? "",
    );

    // THE CONTROL FIRST: the sample carries three different origins on purpose.
    // A column that drew one heading, or none, would satisfy a weaker check.
    expect(groups.length, "the column drew no origin headings at all").toBeGreaterThan(1);
    expect(new Set(groups).size, "two headings with the same name").toBe(groups.length);

    // And every flow is inside one of them: a row outside every group is a flow
    // whose origin the window received and dropped.
    const inside = container.querySelectorAll(".rail__group .rail__item").length;
    const all = container.querySelectorAll(".rail__item").length;
    expect(all, "the column has no flows to group").toBeGreaterThan(0);
    expect(inside, `${all - inside} rows sit outside every group`).toBe(all);
  });
});
