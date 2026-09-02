// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { Palette, isPaletteKey, matching, type Entry } from "./Palette";

/**
 * **ONE LINE OF TYPING REACHES ANY PLACE AND ANY FLOW.** The palette lists what
 * it is handed and runs the entry picked; it invents nothing and opens nothing
 * by itself. The absurd control: typing what matches nothing runs nothing.
 */

afterEach(cleanup);

function entries(seen: string[]): Entry[] {
  return [
    { group: "Go to", label: "Board", hint: "what am I doing", run: () => seen.push("go board") },
    { group: "Go to", label: "Memory › Ledger", hint: "the tables, as they are", run: () => seen.push("go ledger") },
    { group: "Open flow", label: "relay", hint: "yours", run: () => seen.push("open relay") },
    { group: "Run flow", label: "relay", hint: "yours", run: () => seen.push("run relay") },
  ];
}

describe("the palette", () => {
  test("matching takes every word typed, in any order, over group, label and hint", () => {
    const all = entries([]);
    expect(matching(all, "").map((one) => one.label)).toHaveLength(4);
    expect(matching(all, "run relay").map((one) => one.group)).toEqual(["Run flow"]);
    expect(matching(all, "ledger").map((one) => one.label)).toEqual(["Memory › Ledger"]);
    expect(matching(all, "yours").map((one) => one.group)).toEqual(["Open flow", "Run flow"]);
    expect(matching(all, "nothing of the sort")).toEqual([]);
  });

  test("⌘K and Ctrl+K are the key, and K alone is not", () => {
    expect(isPaletteKey({ key: "k", metaKey: true, ctrlKey: false })).toBe(true);
    expect(isPaletteKey({ key: "K", metaKey: false, ctrlKey: true })).toBe(true);
    expect(isPaletteKey({ key: "k", metaKey: false, ctrlKey: false })).toBe(false);
    expect(isPaletteKey({ key: "j", metaKey: true, ctrlKey: false })).toBe(false);
  });

  test("TYPING NARROWS, ENTER RUNS THE ONE UNDER THE CURSOR, and the palette closes", () => {
    const seen: string[] = [];
    const onClose = vi.fn();
    render(<Palette entries={entries(seen)} open onClose={onClose} />);
    const box = screen.getByPlaceholderText("Search or run a command");
    fireEvent.change(box, { target: { value: "relay" } });
    expect(screen.getAllByRole("option")).toHaveLength(2);
    fireEvent.keyDown(box, { key: "ArrowDown" });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(seen).toEqual(["run relay"]);
    expect(onClose).toHaveBeenCalled();
  });

  test("what matches nothing runs nothing, and says so", () => {
    const seen: string[] = [];
    render(<Palette entries={entries(seen)} open onClose={() => {}} />);
    const box = screen.getByPlaceholderText("Search or run a command");
    fireEvent.change(box, { target: { value: "zzz" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(seen).toEqual([]);
    expect(screen.getByText(/Nothing matches «zzz»/)).toBeTruthy();
  });

  test("closed, it draws nothing", () => {
    const { container } = render(<Palette entries={entries([])} open={false} onClose={() => {}} />);
    expect(container.querySelector(".palette")).toBeNull();
  });
});
