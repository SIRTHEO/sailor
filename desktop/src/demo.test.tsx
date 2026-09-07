// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Demo, LINE_MS, STEP_MS, type DemoStep } from "./Demo";

afterEach(cleanup);
afterEach(() => vi.useRealTimers());

const SCRIPT: DemoStep[] = [
  { id: "one", title: "First step", lines: ["> one"] },
  { id: "two", title: "Second step", lines: ["> two", "> two again"] },
];

/**
 * **CLOSED MEANS ABSENT, NOT HIDDEN.** A dialog left in the tree under
 * `display: none` still answers to a query by role; a person using a screen
 * reader would find a demonstration nobody asked for.
 */
test("CLOSED RENDERS NOTHING", () => {
  render(<Demo open={false} onClose={() => {}} script={SCRIPT} />);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("OPEN SHOWS THE START BUTTON, AND NO STEP HAS RUN YET", () => {
  render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  expect(screen.getByRole("button", { name: "Start a demonstration" })).toBeTruthy();
  expect(screen.queryByText("First step")).toBeNull();
});

test("STARTING PLAYS THE STEPS IN ORDER, ONE LINE AT A TIME", () => {
  vi.useFakeTimers();
  render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  act(() => screen.getByRole("button", { name: "Start a demonstration" }).click());

  const first = () => screen.getByText("First step").closest(".demo__step");
  expect(first()?.getAttribute("data-state"), "the first step starts working at once").toBe("working");
  expect(first()?.querySelector(".demo__lines")?.textContent).toBe("");

  act(() => vi.advanceTimersByTime(LINE_MS));
  expect(first()?.querySelector(".demo__lines")?.textContent).toBe("> one");

  act(() => vi.advanceTimersByTime(STEP_MS));
  const second = () => screen.getByText("Second step").closest(".demo__step");
  expect(first()?.getAttribute("data-state"), "the first step is done once the second begins").toBe("done");
  expect(second()?.getAttribute("data-state")).toBe("working");
});

test("THE SCRIPT ENDS WITH A CLOSING LINE, NOT A LOOP", () => {
  vi.useFakeTimers();
  render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  act(() => screen.getByRole("button", { name: "Start a demonstration" }).click());
  act(() => vi.advanceTimersByTime(SCRIPT.length * STEP_MS));
  expect(screen.getByText(/scripted run, not a real one/)).toBeTruthy();
});

test("CLOSING AND REOPENING STARTS FROM THE BEGINNING, NOT MID-SCRIPT", () => {
  vi.useFakeTimers();
  const { rerender } = render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  act(() => screen.getByRole("button", { name: "Start a demonstration" }).click());
  act(() => vi.advanceTimersByTime(LINE_MS));

  rerender(<Demo open={false} onClose={() => {}} script={SCRIPT} />);
  rerender(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  expect(screen.getByRole("button", { name: "Start a demonstration" })).toBeTruthy();
});

test("THE CLOSE BUTTON CALLS BACK, AND NOTHING ELSE DECIDES TO CLOSE IT", () => {
  const onClose = vi.fn();
  render(<Demo open={true} onClose={onClose} script={SCRIPT} />);
  screen.getByRole("button", { name: "Close the demonstration" }).click();
  expect(onClose).toHaveBeenCalledOnce();
});

test("UNMOUNTING MID-SCRIPT CANCELS EVERY PENDING TIMER, NOT JUST THE VIEW", () => {
  vi.useFakeTimers();
  const { unmount } = render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  act(() => screen.getByRole("button", { name: "Start a demonstration" }).click());
  expect(vi.getTimerCount(), "the script schedules its own timers").toBeGreaterThan(0);
  unmount();
  expect(vi.getTimerCount(), "unmount left a timer that would fire on nothing").toBe(0);
});

test("CLOSING MID-SCRIPT CANCELS THE PENDING TIMERS TOO", () => {
  vi.useFakeTimers();
  const { rerender } = render(<Demo open={true} onClose={() => {}} script={SCRIPT} />);
  act(() => screen.getByRole("button", { name: "Start a demonstration" }).click());
  expect(vi.getTimerCount(), "the script schedules its own timers").toBeGreaterThan(0);
  act(() => rerender(<Demo open={false} onClose={() => {}} script={SCRIPT} />));
  expect(vi.getTimerCount(), "closing left a timer that would still land on the next play").toBe(0);
});
