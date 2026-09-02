// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { KeepsScreen } from "./KeepsScreen";
import { sizeWords } from "./keeps";

/**
 * **A MISSING STORE IS SAID, NOT ZEROED.** The screen draws the engine's rows
 * with their real paths; a store that does not exist yet gets the sentence,
 * not a plausible count, and the version in service says where the binary is
 * and what it was built from.
 */

afterEach(cleanup);

function pretendShell(answers: Record<string, unknown>): () => void {
  const before = (window as unknown as { __TAURI__?: unknown }).__TAURI__;
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) =>
        command in answers ? Promise.resolve(answers[command]) : Promise.reject(new Error(`no ${command}`)),
    },
  };
  return () => {
    (window as unknown as { __TAURI__?: unknown }).__TAURI__ = before;
  };
}

describe("what Sailor keeps", () => {
  test("sizes read as a person reads them", () => {
    expect(sizeWords(512)).toBe("512 B");
    expect(sizeWords(344 * 1024)).toBe("344 KB");
    expect(sizeWords(1.5 * 1024 * 1024)).toBe("1.5 MB");
    expect(sizeWords(24 * 1024 * 1024 * 1024)).toBe("24.0 GB");
  });

  test("THE ROWS ARE THE ENGINE'S, and the missing store gets the sentence", async () => {
    const stop = pretendShell({
      what_sailor_keeps: {
        home: "/home/theo/.config/sailor",
        home_files: 96,
        home_bytes: 1024 * 1024,
        stores: [
          { what: "Flows, yours", where: "/home/theo/.config/sailor/flows", how_many: 21, bytes: 344 * 1024, exists: true },
          { what: "Runs, steps and events", where: "/home/theo/.config/sailor/ledger", how_many: null, bytes: null, exists: false },
        ],
        in_service: { binary: "/home/theo/.local/bin/sailor", built_at: 1_788_365_000, commit: "5742da24aa8e", window_version: "0.1.0" },
        project_root: "/home/theo/personal/sailor",
      },
    });
    try {
      render(<KeepsScreen native />);
      await screen.findByText("Flows, yours");
      expect(screen.getByText("/home/theo/.config/sailor/flows")).toBeTruthy();
      expect(screen.getByText("21")).toBeTruthy();
      expect(screen.getByText("344 KB")).toBeTruthy();
      expect(screen.getByText(/not created yet/)).toBeTruthy();
      expect(screen.getByText("/home/theo/.local/bin/sailor")).toBeTruthy();
      expect(screen.getByText("sources 5742da24")).toBeTruthy();
      expect(screen.getByText("/home/theo/personal/sailor")).toBeTruthy();
      expect(screen.getByText(/1\.0 MB · 96 files/)).toBeTruthy();
    } finally {
      stop();
    }
  });

  test("an engine that cannot answer is said, not passed for an empty home", async () => {
    const stop = pretendShell({});
    try {
      render(<KeepsScreen native />);
      await screen.findByText(/no what_sailor_keeps/);
      expect(screen.queryByText(/not created yet/)).toBeNull();
    } finally {
      stop();
    }
  });
});
