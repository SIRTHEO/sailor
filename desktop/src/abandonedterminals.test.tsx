// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { WaitingScreen } from "./WaitingScreen";
import { abandonedDecisions, blindSpots, type Sources } from "./waiting";
import type { Abandoned } from "./terminal";

/**
 * **A TERMINAL KILLED WITHOUT CLOSING STAYS OPEN FOR EVER**, by design: it is a
 * fact to show, not to hide. Fault 126 — the window listed it as open and gave
 * no way to learn otherwise, because no command answered for the census.
 */

afterEach(cleanup);

const NOW = 1_000_000;

function sources(terminals: Sources["terminals"]): Sources {
  return {
    open: { state: "answered", value: [] },
    handed: { state: "answered", value: {} },
    history: { state: "answered", value: [] },
    quota: { state: "answered", value: { windows: [], unreachable: [] } },
    terminals,
  };
}

const seen = (ttys: string[]): Abandoned => ({ answer: "seen", ttys });
const refused: Abandoned = {
  answer: "could_not_look",
  refusal: { tool: "ps", reason: "operation not permitted" },
};

describe("a terminal the register calls open that holds nobody", () => {
  test("IS ONE OF THE THINGS WAITING FOR YOU, named by its tty", () => {
    render(<WaitingScreen native now={NOW} since={NOW - 3600} sources={sources({ state: "answered", value: seen(["ttys009"]) })} />);
    expect(screen.getByText(/ttys009/)).toBeTruthy();
    expect(screen.getByText("1 thing waits for you")).toBeTruthy();
  });

  test("A MACHINE NOBODY COULD QUESTION IS A HOLE IN THE COUNT, not a clean register", () => {
    const asked = sources({ state: "answered", value: refused });
    expect(abandonedDecisions(asked.terminals), "nothing may be claimed of what was not looked at").toEqual([]);
    expect(blindSpots(asked).join(" "), "and the hole has to be named").toContain("ps");
    render(<WaitingScreen native now={NOW} since={NOW - 3600} sources={asked} />);
    expect(
      screen.getByText("Nothing I could read is waiting for you"),
      "an unquestioned machine must not read as an empty morning",
    ).toBeTruthy();
  });

  test("and a register with nothing abandoned says nothing at all", () => {
    expect(abandonedDecisions({ state: "answered", value: seen([]) })).toEqual([]);
    expect(blindSpots(sources({ state: "answered", value: seen([]) }))).toEqual([]);
  });

  test("nothing dates a killed terminal, so the row invents no moment", () => {
    const [row] = abandonedDecisions({ state: "answered", value: seen(["ttys009"]) });
    expect(row?.since, "an invented «waiting for 3h» would be a fact nobody wrote").toBeNull();
  });
});
