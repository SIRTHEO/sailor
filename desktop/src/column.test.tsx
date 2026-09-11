// @vitest-environment jsdom
/** THE COLUMN SAYS WHERE ITS PARTS BEGIN. Three groups in one nave under
 * plain text gave a scan nothing to land on and a reader one run-on string,
 * so the headings are asserted as headings. */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { World } from "./World";
import { theColumn, EMPTY_COLUMN, type ColumnReading } from "./column";

function paint(globalFlows?: React.ReactNode) {
  cleanup();
  return render(
    <World
      native={false}
      source="sample"
      here="waiting"
      onGo={() => {}}
      counts={{}}
      terminals={[]}
      onMoved={() => {}}
      onTree={() => {}}
      flowGroups={[]}
      focusName={null}
      onFlow={() => {}}
      onNewFlow={() => {}}
      globalFlows={globalFlows}
    />,
  );
}

afterEach(cleanup);

describe("the column names its own parts", () => {
  test("EACH GROUP OF THE COLUMN IS A HEADING, NOT A LINE OF TEXT", () => {
    paint();
    const heads = screen.getAllByRole("heading").map((one) => one.textContent);
    expect(heads).toContain("workspaces");
    expect(heads).toContain("outside every workspace");
  });

  test("A GROUP IS NAMED BY ITS OWN HEADING", () => {
    paint();
    expect(screen.getByRole("group", { name: "outside every workspace" })).toBeTruthy();
    expect(screen.getByRole("group", { name: "workspaces" })).toBeTruthy();
  });

  test("THE LINKED GLOBAL FLOWS HAVE A SLOT, AND SOMEBODY ELSE FILLS IT", () => {
    paint(<p>the linked flows, everywhere</p>);
    expect(screen.getByText("the linked flows, everywhere")).toBeTruthy();
  });
});

describe("the one reading the column is fed from", () => {
  test("THE STUB ANSWERS THE DECLARED SHAPE", async () => {
    const reading: ColumnReading = await theColumn();
    expect(reading.workspaces.length).toBeGreaterThan(0);
    expect(reading.workspaces[0].trees.length).toBeGreaterThan(0);
    expect(reading.workspaces[0].trees[0].flows.length).toBeGreaterThan(0);
    expect(reading.flowsEverywhere.map((one) => one.origin)).toContain("built in");
    expect(reading.outside.terminals.length).toBeGreaterThan(0);
    expect(reading.standingIn).not.toBeNull();
  });

  test("NOTHING READ YET IS A SHAPE, NOT AN ABSENCE", () => {
    expect(EMPTY_COLUMN.workspaces).toEqual([]);
    expect(EMPTY_COLUMN.flowsEverywhere).toEqual([]);
    expect(EMPTY_COLUMN.outside.terminals).toEqual([]);
    expect(EMPTY_COLUMN.standingIn).toBeNull();
  });
});
