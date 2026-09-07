// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { Calls, identityReading } from "./History";
import type { EngineIdentity, ModelCall } from "./engine";
const crates = import.meta.glob("../../crates/ledger/src/identity.rs", {
  query: "?raw",
  import: "default",
  eager: true,
});
const identitySource = Object.values(crates)[0] as string;
const readers = import.meta.glob("./History.tsx", { query: "?raw", import: "default", eager: true });
const reader = Object.values(readers)[0] as string;

/**
 * **IT TRAVELS THIS FAR SO THAT SOMEBODY LOOKS AT IT** — the crate's own words
 * about `engine_identity`, which reached the window and no screen at all.
 */

const call = (identity: EngineIdentity): ModelCall => ({
  call_id: "c", step_id: null, purpose: "p", cli: "e",
  requested_model: "", actual_model: "m",
  input_tokens: 1, output_tokens: 1, cached_tokens: null,
  cache_write_tokens: null, cache_write_long_tokens: null, total_tokens: null,
  turns: null, cost_micros: null, declared_cost_micros: null,
  error_type: null, started_at: 1, ended_at: 2, engine_identity: identity,
});

describe("the identity a call was paid under", () => {
  test("EVERY CASE THE CRATE WRITES IS A CASE THIS WINDOW READS", () => {
    const written = [...identitySource.matchAll(/^ {4}([A-Z]\w+)\s*[{,]/gm)].map((one) =>
      one[1].replace(/(?<!^)([A-Z])/g, "_$1").toLowerCase(),
    );
    // The control: a parse that found nothing would guard nothing. Asked of the
    // source and not of a fixture, because a fixture for one variant says
    // nothing about the others and a half-built one accuses the reader.
    expect(written.length, "no variant parsed out of the crate").toBeGreaterThan(5);
    for (const kind of written) {
      expect(reader, `«${kind}» crosses the wire and the window has no case for it`).toContain(
        `case "${kind}":`,
      );
    }
    for (const [, kind] of reader.matchAll(/case "(\w+)":/g)) {
      expect(written, `the window reads «${kind}», the crate writes no such case`).toContain(kind);
    }
  });

  test("A PROFILE THE LIST NO LONGER HAS IS THE ONE THAT ASKS FOR A HAND", () => {
    expect(identityReading({ kind: "profile_vanished", cli_id: "e", profile_name: "work" })).toEqual({
      said: "«work» is gone from the list",
      asks: true,
    });
    for (const kind of ["inherited_from_the_terminal", "not_a_known_engine", "declared_by_an_agent"] as const) {
      expect(
        identityReading({ kind } as EngineIdentity).asks,
        `«${kind}» is a decision, and marking it teaches a reader to ignore the mark`,
      ).toBe(false);
    }
  });

  test("and an endpoint of its own is said, because it says who was charged", () => {
    const away = identityReading({
      kind: "profile_in_force", cli_id: "e", profile_name: "work",
      home_dir: "/somewhere", endpoint: "https://an-endpoint",
    });
    expect(away.said).toContain("an-endpoint");
    const home = identityReading({
      kind: "profile_in_force", cli_id: "e", profile_name: "work", home_dir: "/somewhere",
    });
    expect(home.said, "a profile on its maker's endpoint says only its name").toBe("work");
  });

  test("THE TABLE SHOWS IT", () => {
    render(<Calls calls={[call({ kind: "profile_vanished", cli_id: "e", profile_name: "work" })]} />);
    expect(screen.getByText(/gone from the list/)).toBeTruthy();
  });

  test("A SHAPE IT CANNOT READ IS NOT A WHITE SCREEN, and not «not recorded» either", () => {
    expect(identityReading(undefined).said).toContain("no words here");
    expect(identityReading({ kind: "something-later" } as unknown as EngineIdentity).said).toContain(
      "no words here",
    );
    expect(
      identityReading({ kind: "unrecorded", legacy: "" }).said,
      "a row with no identity recorded and one this side cannot read are two facts",
    ).not.toContain("no words here");
  });

  test("a row from before the field says so, and never invents a profile", () => {
    expect(identityReading({ kind: "unrecorded", legacy: "" }).said).toBe("not recorded");
    const old = identityReading({ kind: "unrecorded", legacy: "work" }).said;
    expect(old, "the old column could already lie, so it is quoted, not promoted").toContain("not recorded");
  });
});
