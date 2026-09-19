// @vitest-environment jsdom
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Strip } from "./Strip";
import type { StripAccount, StripRun } from "./engine";

afterEach(cleanup);

const NOW = 1726224000;

function makeRun(id: string, entity: string, state: "waiting" | "working" | "not_yet" | "holder_gone", opts: Partial<StripRun> = {}): StripRun {
  return {
    run_id: id,
    entity,
    state,
    step: opts.step ?? null,
    elapsed_secs: opts.elapsed_secs ?? 60,
    cap_micros: opts.cap_micros ?? null,
    spend_micros: opts.spend_micros ?? 0,
    device: opts.device ?? null,
    holder_gone: opts.holder_gone ?? (state === "holder_gone"),
  };
}

function makeAccount(name: string, cli_id: string, monogram: string, unavailable: boolean, spent_fraction: number | null = null): StripAccount {
  return {
    name,
    cli_id,
    monogram,
    unavailable,
    spent_fraction,
    resets_at: null,
    read_at: unavailable ? null : NOW - 120,
    unit: "five_hour",
  };
}

describe("Strip", () => {
  test("zero runs at rest renders clear word and ended today", () => {
  const accounts = [
    makeAccount("profile-one", "claude", "CL1", false, 0.45),
  ];
  render(
    <Strip
      runs={[]}
      accounts={accounts}
      endedToday={3}
      now={NOW}
    />
  );

  expect(screen.getByText("clear")).toBeDefined();
  expect(screen.getByText("3 ended today")).toBeDefined();
  expect(screen.queryByTestId("strip-run-cell")).toBeNull();
});

  test("four runs and two more shows four runs plus +2 more button", () => {
  const runs = [
    makeRun("run-1", "dispatch", "working"),
    makeRun("run-2", "review", "working"),
    makeRun("run-3", "test", "working"),
    makeRun("run-4", "lint", "working"),
    makeRun("run-5", "build", "working"),
    makeRun("run-6", "deploy", "working"),
  ];
  const onOpenAttention = vi.fn();
  render(
    <Strip
      runs={runs}
      accounts={[]}
      endedToday={0}
      now={NOW}
      onOpenAttention={onOpenAttention}
    />
  );

  const runCells = screen.getAllByTestId("strip-run-cell");
  expect(runCells.length).toBe(4);

  const moreButton = screen.getByRole("button", { name: /\+2 more/i });
  expect(moreButton).toBeDefined();

  fireEvent.click(moreButton);
  expect(onOpenAttention).toHaveBeenCalledTimes(1);
});

  test("an account with unavailable true renders no fill and the word", () => {
  const accounts = [
    makeAccount("available-profile", "claude", "CL1", false, 0.5),
    makeAccount("unavailable-profile", "antigravity", "AG1", true, null),
  ];
  render(
    <Strip
      runs={[]}
      accounts={accounts}
      endedToday={0}
      now={NOW}
    />
  );

  expect(screen.getByText("unavailable")).toBeDefined();

  const unavailableCell = screen.getByTestId("strip-account-AG1");
  expect(unavailableCell.querySelector(".strip__track_fill")).toBeNull();

  const availableCell = screen.getByTestId("strip-account-CL1");
  expect(availableCell.querySelector(".strip__track_fill")).not.toBeNull();
});

  test("the two halves never share a number", () => {
  const runs = [
    makeRun("run-1", "dispatch", "working", { spend_micros: 250_000, cap_micros: 1_000_000 }),
  ];
  const accounts = [
    makeAccount("profile-one", "codex", "CX1", false, 0.75),
  ];
  const { container } = render(
    <Strip
      runs={runs}
      accounts={accounts}
      endedToday={0}
      now={NOW}
    />
  );

  const rule = container.querySelector(".strip__rule");
  expect(rule).not.toBeNull();

  const leftHalf = container.querySelector(".strip__runs");
  const rightHalf = container.querySelector(".strip__accounts");

  expect(leftHalf).not.toBeNull();
  expect(rightHalf).not.toBeNull();

  const leftText = leftHalf?.textContent ?? "";
  const rightText = rightHalf?.textContent ?? "";

  expect(leftText).toContain("$0.25");
  expect(rightText).not.toContain("$0.25");
  expect(container.textContent).not.toMatch(/health|score|ratio|total/i);
});

  test("none of the prohibited elements are on the strip", () => {
  const runs = [
    makeRun("run-1", "dispatch", "waiting", { step: "approve" }),
    makeRun("run-2", "worker", "not_yet"),
  ];
  const accounts = [
    makeAccount("profile-one", "claude", "CL1", false, 0.8),
  ];
  const { container } = render(
    <Strip
      runs={runs}
      accounts={accounts}
      endedToday={2}
      now={NOW}
    />
  );

  const text = container.textContent ?? "";
  expect(text).not.toMatch(/\b(health score|health|score)\b/i);
  expect(text).not.toMatch(/\b(completion %|% complete|progress %)\b/i);
  expect(text).not.toMatch(/\b(guaranteed money|guaranteed remaining)\b/i);
  expect(text).not.toMatch(/\b(tokens\/s|tok\/s|tokens per second)\b/i);

  const rings = container.querySelectorAll(".ring, [data-ring], svg circle");
  expect(rings.length).toBe(0);

  const animatedPresence = container.querySelectorAll(".pulse, .animate-spin, .animate-pulse");
  expect(animatedPresence.length).toBe(0);
});
});
