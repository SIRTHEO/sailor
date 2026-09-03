// @vitest-environment jsdom
/**
 * **A MENU THAT RENDERS NOTHING IS WORSE THAN A BUTTON.** Deleting a flow left
 * the bar for a mark one click away; if that mark opens an empty portal the
 * gesture is gone and the screen looks tidier for it.
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import App from "./App";

afterEach(cleanup);

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
  // Radix asks the element about pointer capture, and jsdom has no answer.
  Element.prototype.hasPointerCapture = () => false;
  Element.prototype.setPointerCapture = () => {};
  Element.prototype.releasePointerCapture = () => {};
  Element.prototype.scrollIntoView = () => {};
});

describe("the flow's own menu", () => {
  test("THE MARK OPENS A MENU, AND DELETE IS IN IT", () => {
    const { container } = render(<App />);
    screen.getByRole("button", { name: /^Board/ }).click();
    fireEvent.click(container.querySelector("button.rail__item") as HTMLElement);

    const more = screen.getByRole("button", { name: "more for this flow" });
    // THE CONTROL FIRST: closed, the word is nowhere on the screen.
    expect(screen.queryByRole("menuitem", { name: /flow$/ })).toBeNull();

    fireEvent.pointerDown(more, { button: 0, ctrlKey: false, pointerType: "mouse" });
    expect(screen.getByRole("menuitem", { name: /^(Delete|Discard) flow$/ })).toBeTruthy();
  });
});
