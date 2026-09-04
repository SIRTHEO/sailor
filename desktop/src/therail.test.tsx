// @vitest-environment jsdom
/**
 * **THE COLUMN GROUPS FLOWS BY WHERE THEY COME FROM.** Three sources reach the
 * window, and flat they were indistinguishable. On a name clash the most
 * specific wins, so the group also answers which of two namesakes runs.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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

/** The board is one place among several, and a click is what opens it. */
function goToFlows(): void {
  screen.getByRole("button", { name: /^Board/ }).click();
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


/**
 * **THE BOARD IS NEVER EMPTY WHILE THERE IS A FLOW TO DRAW.** «None in focus»
 * used to mean «all of them, faded»; with one flow on the paper it would mean a
 * blank board, which is nothing to go back to.
 */
describe("the flow the board opens on", () => {
  test("AT REST ONE ROW IS ALREADY OPEN, and one flow is on the paper", () => {
    const { container } = render(<App />);
    goToFlows();

    const open = container.querySelectorAll("button.rail__item[data-open]");
    expect(container.querySelectorAll("button.rail__item").length, "the column has no flows").toBeGreaterThan(1);
    expect(open, "no row is open, or two are").toHaveLength(1);
    expect(container.querySelectorAll(".flow-band"), "the paper holds none, or more than one").toHaveLength(1);
  });

  test("PRESSING THE OPEN ROW KEEPS IT OPEN, it does not send you back to the first", () => {
    const { container } = render(<App />);
    goToFlows();

    // NOT THE ROW THE BOARD OPENED ON: there, keeping the flow and dropping it
    // for the board to re-open its first look the same.
    const rows = Array.from(container.querySelectorAll<HTMLElement>("button.rail__item"));
    const row = rows.find((one) => one.getAttribute("data-open") === null) as HTMLElement;
    expect(row, "the column offers no second flow").toBeDefined();
    fireEvent.click(row);

    const name = row.querySelector(".rail__label")?.textContent ?? "";
    expect(name, "the row has no name").not.toBe("");
    expect(row.getAttribute("data-open"), "the press did not open it").not.toBeNull();

    fireEvent.click(row);

    const stillOpen = container.querySelectorAll("button.rail__item[data-open]");
    expect(stillOpen, "the press closed the flow, or opened a second").toHaveLength(1);
    expect(
      stillOpen[0].querySelector(".rail__label")?.textContent,
      "the press moved to another flow",
    ).toBe(name);
    expect(container.querySelectorAll(".flow-band")).toHaveLength(1);
  });
});
