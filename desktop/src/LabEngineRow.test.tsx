// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { LabEngineRow } from "./LabEngineRow";

afterEach(cleanup);

describe("LabEngineRow", () => {
  test("reachable: resident models and response time are said", () => {
    render(
      <LabEngineRow
        ask={{
          state: "asked",
          value: {
            reachable: true,
            resident_models: ["Qwen3.5-4B-Q4_K_M"],
            response_time_secs: 0.079,
            checked_at: "2026-09-14T09:49:25Z",
            error: null,
          },
        }}
      />,
    );
    expect(screen.getByText("answers")).toBeTruthy();
    expect(screen.getByText("Qwen3.5-4B-Q4_K_M")).toBeTruthy();
    expect(screen.getByText("0.079s to answer")).toBeTruthy();
  });

  test("not reachable: the error is said, no resident row guessed at", () => {
    render(
      <LabEngineRow
        ask={{
          state: "asked",
          value: {
            reachable: false,
            resident_models: [],
            response_time_secs: null,
            checked_at: "2026-09-14T09:49:25Z",
            error: "connection refused",
          },
        }}
      />,
    );
    expect(screen.getByText("does not answer")).toBeTruthy();
    expect(screen.getByText("connection refused")).toBeTruthy();
    expect(screen.queryByText("resident")).toBeNull();
  });

  test("mute: the reason is said, never a silent gap", () => {
    render(<LabEngineRow ask={{ state: "mute", why: "outside the native shell" }} />);
    expect(screen.getByText(/outside the native shell/)).toBeTruthy();
  });
});
