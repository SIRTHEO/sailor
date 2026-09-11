// @vitest-environment jsdom
/**
 * **A MAP NOBODY CAN OPEN IS A MAP NOBODY HAS.** The reading, the surface and
 * the shell command each passed their own tests while no row led to any of
 * them, so this asks for the path a person actually walks.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";
import { nameOfPlace } from "./places";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

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

describe("the map of the flows has a way in", () => {
  test("THE ROW SITS WITH THE FLOWS THAT ARE THE SAME WHEREVER YOU STAND", () => {
    const { container } = render(<App />);
    const group = screen.getByRole("group", { name: "flows everywhere" });

    const row = [...group.querySelectorAll("button")].find(
      (one) => one.textContent?.includes(nameOfPlace("flowmap")),
    );

    expect(row, "no row of the column opens the map").toBeDefined();
    // The group's own heading says «flows everywhere»: a row repeating it
    // would be the same name written twice in one place.
    expect(row?.textContent).not.toContain("Flows everywhere");
    expect(container.querySelector('[data-place="flowmap"]')).toBeNull();
  });

  test("PRESSING IT OPENS THE MAP, AND THE MAP SAYS WHY IT HAS NOTHING", async () => {
    const { container } = render(<App />);
    const group = screen.getByRole("group", { name: "flows everywhere" });
    const row = [...group.querySelectorAll("button")].find(
      (one) => one.textContent?.includes(nameOfPlace("flowmap")),
    );

    fireEvent.click(row as HTMLElement);

    await waitFor(() => {
      const body = container.querySelector('[data-place="flowmap"] .section__body');
      expect(body, "the row opened nothing").not.toBeNull();
      // Outside the shell there is no engine to ask, and «I could not ask» is
      // the answer that must be drawn: an empty map and an unasked one look
      // the same, and only one of them means there are no flows.
      expect(body?.textContent ?? "").toContain("could not be read");
    });
  });
});
