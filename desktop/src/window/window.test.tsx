// @vitest-environment jsdom
import { afterEach, describe, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Field } from "./Field";
import { Panel } from "./Panel";
import { LISTS } from "./lists";
import { ScopeSwitch } from "./ScopeSwitch";
import { Window } from "./Window";

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
      panel={<Panel title="Workspaces" scope={null}>
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

describe("the scope switch", () => {
  test("OUTSIDE A WORKSPACE IT IS NOT DRAWN, rather than drawn with one side", () => {
    const { container } = render(
      <ScopeSwitch scope="all" inAWorkspace={false} counts={{ here: "0", all: "118" }} onScope={vi.fn()} />,
    );
    expect(container.firstChild).toBeNull();
  });

  test("inside one, each side carries its own count", () => {
    render(
      <ScopeSwitch scope="all" inAWorkspace counts={{ here: "41", all: "118" }} onScope={vi.fn()} />,
    );
    const here = screen.getByRole("button", { name: /Here/ });
    expect(here.getAttribute("aria-pressed")).toBe("false");
    expect(here.textContent).toContain("41");
    expect(screen.getByRole("button", { name: /All/ }).getAttribute("aria-pressed")).toBe("true");
  });
});
