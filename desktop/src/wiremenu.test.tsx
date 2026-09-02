// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { TOOLBAR_KINDS } from "./Toolbar";
import { KIND_LABEL } from "./StepNode";
import { WireMenu } from "./WireMenu";

afterEach(cleanup);

function open(onPick = vi.fn(), onClose = vi.fn()) {
  render(<WireMenu at={{ x: 10, y: 20 }} from="read" onPick={onPick} onClose={onClose} />);
  return { onPick, onClose };
}

describe("the menu a wire opens", () => {
  /**
   * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. Two hand-written lists of
   * the same families drift, and the one nobody looks at stops offering what
   * the other does — it already happened here once, with the node's marks and
   * the bar's. This menu reads the bar's list, and this test refuses a copy.
   */
  test("it offers exactly the families the toolbox offers", () => {
    open();
    const offered = screen.getAllByRole("menuitem").map((item) => item.textContent);
    expect(offered).toEqual(TOOLBAR_KINDS.map((kind) => KIND_LABEL[kind]));
  });

  test("it says which step the new one will follow", () => {
    open();
    expect(screen.getByRole("menu").getAttribute("aria-label")).toContain("read");
  });

  test("picking a family reports that family, once", () => {
    const { onPick } = open();
    fireEvent.click(screen.getAllByRole("menuitem")[0]);
    expect(onPick).toHaveBeenCalledTimes(1);
    expect(onPick).toHaveBeenCalledWith(TOOLBAR_KINDS[0]);
  });

  /* A menu that only closes by choosing makes the wrong choice the cheap one. */
  test("escape closes it without choosing", () => {
    const { onPick, onClose } = open();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
    expect(onPick).not.toHaveBeenCalled();
  });

  test("the groups keep the names a screen reader reads", () => {
    open();
    expect(screen.getAllByRole("group").length).toBeGreaterThan(1);
  });
});
