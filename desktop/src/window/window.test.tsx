// @vitest-environment jsdom
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import stylesheetSource from "../styles.css?raw";
import { parseStylesheet, styleTree } from "../contrast";
import type { Project } from "../workspaces";
import { Field } from "./Field";
import { Panel } from "./Panel";
import { LISTS } from "./lists";
import { Window } from "./Window";
import { WorkspaceRows } from "./WorkspaceRows";

afterEach(cleanup);

/**
 * **THE RENDERED WINDOW IS THE ORACLE** (ADR-003): what a person reaches is
 * what is painted, and a list behind a control nobody can see is not a list.
 */
function shell(place: Parameters<typeof Window>[0]["place"], onPlace = vi.fn()) {
  return render(
    <Window
      place={place}
      onPlace={onPlace}
      panel={<Panel title="Workspaces">
        <div>a-code-project</div>
      </Panel>}
      field={<Field name="work/some-branch" note="3 terminals, 1 page">
        <div>the terminal</div>
      </Field>}
    />,
  );
}

describe("the window stands on three columns", () => {
  test("THE FIVE LISTS ARE ALL REACHABLE WITHOUT OPENING ANYTHING", () => {
    shell("workspaces");
    for (const entry of LISTS) {
      expect(screen.getByRole("button", { name: entry.name })).toBeDefined();
    }
  });

  /** `lists.ts` calls the name «what a pointer resting on the icon says». The
   *  rail is the whole navigation while the window is open, so a claim that
   *  only a screen reader can collect leaves a mouse user clicking to learn. */
  test("A POINTER RESTING ON AN ICON IS TOLD THE NAME, not only a reader", () => {
    shell("workspaces");
    for (const entry of LISTS) {
      expect(screen.getByRole("button", { name: entry.name }).getAttribute("title")).toBe(entry.name);
    }
  });

  test("the list in force is the only one marked, and the mark is not a tint", () => {
    shell("data");
    const marked = screen
      .getAllByRole("button")
      .filter((button) => button.getAttribute("aria-current") !== null);
    expect(marked).toHaveLength(1);
    expect(marked[0].getAttribute("aria-label")).toBe("Data");
  });

  test("choosing a list asks for it, and does not choose it in the column", () => {
    const asked = vi.fn();
    shell("workspaces", asked);
    fireEvent.click(screen.getByRole("button", { name: "Keys" }));
    expect(asked).toHaveBeenCalledWith("keys");
  });

  test("the panel names its list and the field names what is open", () => {
    shell("workspaces");
    expect(screen.getByRole("heading", { name: "Workspaces" })).toBeDefined();
    expect(screen.getByLabelText("work/some-branch")).toBeDefined();
    expect(screen.getByText("3 terminals, 1 page")).toBeDefined();
  });
});

function project(name: string): Project {
  return { root: `/somewhere/${name}`, name, first_seen: 0, last_seen: 0, standing: "declared", current: false };
}

const TWO = [project("a-code-project"), project("lab")];

describe("the row the field is holding", () => {
  /** The sheet says the mark is «a bar and a weight». The bar is written; the
   *  weight has to be read off the cascade, not off the rule it should be in. */
  test("CARRIES A WEIGHT AND NOT ONLY A BAND, and no other row does", () => {
    render(<WorkspaceRows projects={TWO} chosen={TWO[1].root} now={0} onChoose={vi.fn()} />);
    const styles = styleTree(document.querySelector(".window-rows")!, parseStylesheet(stylesheetSource));
    const weights = [...document.querySelectorAll(".window-row")].map((row) => ({
      chosen: row.getAttribute("aria-current") !== null,
      weight: styles.get(row.querySelector(".window-row__name")!)?.declarations.get("font-weight"),
    }));
    expect(weights.filter((one) => one.chosen).map((one) => one.weight)).toEqual(["600"]);
    expect(
      weights.filter((one) => !one.chosen).map((one) => one.weight),
      "a weight on every row marks none of them",
    ).toEqual([undefined]);
  });
});
