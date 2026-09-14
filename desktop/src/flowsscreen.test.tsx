// @vitest-environment jsdom
/**
 * **WHAT RUNS HERE, AND IS ANYTHING REPLACING IT**, drawn from a stubbed shell:
 * the checkout the window stands in, every workspace, a name two checkouts
 * resolve differently, and the states that are said instead of drawn blank.
 */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { FlowsScreen, FlowsView } from "./FlowsScreen";
import { globalRows, groupsHere, type FlowContext, type FlowRow, type FlowsReading } from "./flowsbyworkspace";
import { chainWords } from "./flowchain";
import { t } from "./i18n";

afterEach(cleanup);

const SHIPPED = "(shipped with the product)";
const fileIn = (folder: string, name: string) => `${folder}/${name}${".flow.json"}`;
const HOME = "/nowhere/a-home/flows";
const PLAIN = "/nowhere/a-workspace/plain";
const OVERRIDING = "/nowhere/a-workspace/overriding";
const GONE = "/nowhere/a-workspace/gone";

function row(name: string, winner: FlowRow["winner"], replaced: FlowRow["replaced"], resolvedIn: string | null, steps: number | null = 3): FlowRow {
  return { name, resolved_in: resolvedIn, replaced, winner, steps };
}

const OUTSIDE_OVERRIDE = row("example-flow", { origin: "yours", path: fileIn(HOME, "example-flow") }, [{ origin: "built in", path: SHIPPED }], null, 2);

function reading(): FlowsReading {
  const outside: FlowContext = {
    workspace: null,
    root: null,
    branch: null,
    current: false,
    flows: [
      row("a-shipped-flow", { origin: "built in", path: SHIPPED }, [], null, 5),
      OUTSIDE_OVERRIDE,
      row("a-flow-of-yours", { origin: "yours", path: fileIn(HOME, "a-flow-of-yours") }, [], null, 4),
    ],
  };
  return {
    outside,
    contexts: [
      { workspace: "a-workspace", root: PLAIN, branch: "main", current: true, flows: [] },
      {
        workspace: "a-workspace",
        root: OVERRIDING,
        branch: "work/an-override",
        current: false,
        flows: [
          row(
            "example-flow",
            { origin: "this project", path: fileIn(`${OVERRIDING}/flows`, "example-flow") },
            [
              { origin: "built in", path: SHIPPED },
              { origin: "yours", path: fileIn(HOME, "example-flow") },
            ],
            OVERRIDING,
            7,
          ),
        ],
      },
      { workspace: "a-workspace", root: GONE, branch: null, current: false, flows: [], unreadable: "No such file or directory" },
    ],
  };
}

describe("the flows of this checkout", () => {
  test("THE HEADING IS THE QUESTION THE PLACE ANSWERS", () => {
    render(<FlowsView ask={{ state: "read", reading: reading() }} />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(t("window.flows.asks"));
  });

  test("groups by who owns the winner, and a replacing row wears the column's mark", () => {
    const { container } = render(<FlowsView ask={{ state: "read", reading: reading() }} />);
    const groups = [...container.querySelectorAll(".flows__group")].map((one) => one.textContent);
    expect(groups).toEqual([t("window.flows.group.yours"), t("window.flows.group.built_in")]);

    const mark = container.querySelector(".rail__replaces");
    expect(mark?.textContent).toBe(t("window.flow.replaces", { replaced: "built in" }));
    expect(mark?.getAttribute("title")).toBe(chainWords(OUTSIDE_OVERRIDE));
    expect(screen.getByText(/a-workspace · plain · main/)).toBeTruthy();
  });

  test("«of this workspace» comes first where the checkout decides a name", () => {
    const standing = reading();
    standing.contexts[0].current = false;
    standing.contexts[1].current = true;
    expect(groupsHere(standing).map((group) => group.key)).toEqual(["workspace", "yours", "built in"]);
  });

  test("the detail says where the file lives and offers the existing gestures", () => {
    const opened: string[] = [];
    const ran: string[] = [];
    render(
      <FlowsView
        ask={{ state: "read", reading: reading() }}
        onOpen={(name) => opened.push(name)}
        onRun={(name) => ran.push(name)}
      />,
    );
    fireEvent.click(screen.getByText("a-flow-of-yours"));
    const detail = screen.getByRole("complementary", { name: t("window.flows.detail.label") });
    expect(within(detail).getByText(fileIn(HOME, "a-flow-of-yours"))).toBeTruthy();
    fireEvent.click(within(detail).getByRole("button", { name: t("window.flows.detail.open") }));
    fireEvent.click(within(detail).getByRole("button", { name: t("window.flows.detail.run") }));
    expect(opened).toEqual(["a-flow-of-yours"]);
    expect(ran).toEqual(["a-flow-of-yours"]);
  });
});

describe("the flows of every workspace", () => {
  test("A NAME TWO CHECKOUTS RESOLVE DIFFERENTLY EXPANDS INTO EACH WINNER", () => {
    const { container } = render(<FlowsView ask={{ state: "read", reading: reading() }} />);
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));

    const expand = screen.getByRole("button", { name: t("window.flows.differs", { count: 3 }) });
    expect(expand.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(expand);

    const subs = [...container.querySelectorAll(".flows__sub")].map((one) => one.textContent);
    expect(subs).toEqual([
      t("window.flows.outside"),
      "a-workspace · plain · main",
      "a-workspace · overriding · work/an-override",
    ]);
    const origins = [...container.querySelectorAll("tr")]
      .filter((one) => one.querySelector(".flows__sub"))
      .map((one) => one.querySelector(".flows__origin")?.textContent);
    expect(origins).toEqual(["yours", "yours", "this project"]);
  });

  test("a name every context resolves alike stays one row", () => {
    const rows = globalRows(reading());
    const shipped = rows.find((one) => one.name === "a-shipped-flow");
    expect(shipped?.differs).toBe(false);
    expect(shipped?.winners).toHaveLength(3);
  });

  test("a checkout that could not be read says so, with its reason", () => {
    render(<FlowsView ask={{ state: "read", reading: reading() }} />);
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));
    expect(
      screen.getByText(t("window.flows.context_unreadable", { root: GONE, why: "No such file or directory" })),
    ).toBeTruthy();
  });

  test("a file of another checkout offers no gesture that would act on this one", () => {
    const { container } = render(<FlowsView ask={{ state: "read", reading: reading() }} onOpen={() => {}} onRun={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.differs", { count: 3 }) }));
    const overriding = [...container.querySelectorAll("tr")].find((one) =>
      one.querySelector(".flows__sub")?.textContent?.includes("overriding"),
    );
    fireEvent.click(overriding as HTMLElement);
    const detail = screen.getByRole("complementary", { name: t("window.flows.detail.label") });
    expect(within(detail).queryByRole("button")).toBeNull();
    expect(within(detail).getByText(t("window.flows.detail.elsewhere"))).toBeTruthy();
  });
});

describe("the states are said, not drawn as nothing", () => {
  test("a reading the shell refused shows its reason", () => {
    render(<FlowsView ask={{ state: "unreadable", why: "the register would not open" }} />);
    expect(screen.getByRole("alert").textContent).toBe(
      t("window.flows.unreadable", { why: "the register would not open" }),
    );
    expect(screen.getByRole("heading", { level: 1 })).toBeTruthy();
  });

  test("no workspace and no flow at all are two sentences", () => {
    const empty: FlowsReading = {
      outside: { workspace: null, root: null, branch: null, current: false, flows: [] },
      contexts: [],
    };
    render(<FlowsView ask={{ state: "read", reading: empty }} />);
    expect(screen.getByText(t("window.flows.no_workspace"))).toBeTruthy();
    expect(screen.getByText(t("window.flows.no_flows"))).toBeTruthy();
    expect(screen.getByText(t("window.flows.outside"))).toBeTruthy();
  });

  test("outside the native shell the place says there is no disk to read", () => {
    render(<FlowsScreen native={false} />);
    expect(screen.getByRole("alert").textContent).toBe(
      t("window.flows.unreadable", { why: t("window.flows.no_shell") }),
    );
  });
});
