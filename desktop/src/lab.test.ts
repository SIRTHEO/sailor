import { describe, expect, test, vi } from "vitest";

vi.mock("./ledger", () => ({ ledgerQuery: vi.fn() }));

import { ledgerQuery } from "./ledger";
import { labStatus, residentWords, responseWords } from "./lab";

describe("labStatus", () => {
  test("no row ever written is null, not a false reachable", async () => {
    vi.mocked(ledgerQuery).mockResolvedValue({ columns: ["value"], rows: [], truncated: false });
    expect(await labStatus()).toBeNull();
  });

  test("the newest row, parsed from its JSON text", async () => {
    const row = {
      reachable: true,
      resident_models: ["Qwen3.5-4B-Q4_K_M"],
      response_time_secs: 0.079,
      checked_at: "2026-09-14T09:49:25Z",
      error: null,
    };
    vi.mocked(ledgerQuery).mockResolvedValue({ columns: ["value"], rows: [[JSON.stringify(row)]], truncated: false });
    expect(await labStatus()).toEqual(row);
  });

  test("a row of the wrong shape is refused, not guessed at", async () => {
    vi.mocked(ledgerQuery).mockResolvedValue({ columns: ["value"], rows: [[JSON.stringify({ nope: true })]], truncated: false });
    await expect(labStatus()).rejects.toThrow(/does not carry the shape/);
  });
});

describe("residentWords", () => {
  test("nothing resident is said, not a blank line", () => {
    expect(residentWords([])).toBe("nothing resident");
  });

  test("more than one model, joined", () => {
    expect(residentWords(["a", "b"])).toBe("a, b");
  });
});

describe("responseWords", () => {
  test("unknown is said, never a bare dash", () => {
    expect(responseWords(null)).toBe("response time unknown");
  });

  test("seconds to three places", () => {
    expect(responseWords(0.0789)).toBe("0.079s to answer");
  });
});
