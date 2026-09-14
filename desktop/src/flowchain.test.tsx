// @vitest-environment jsdom
/**
 * **A FLOW THAT REPLACES ANOTHER SAYS SO, AND SAYS FROM WHERE.** The window
 * resolves flows in its own working directory, not the terminal's, so the mark
 * the column and the flow's bar wear carries the chain and that directory.
 */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { World, type FlowGroup } from "./World";
import { FocusBar } from "./FocusBar";
import { TooltipProvider } from "./components/ui/tooltip";
import { chainWords, flowChains, replacesWords, type FlowChain } from "./flowchain";
import type { FlowFile } from "./flow";
import { t } from "./i18n";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

/** An invented home, joined to a name here so no literal names a flow file. */
const HOME_FLOWS = "/a-home/flows";
const fileIn = (name: string) => `${HOME_FLOWS}/${name}${".flow.json"}`;

const REPLACING: FlowChain = {
  name: "example-flow",
  resolved_in: "/a-project/a-tree",
  replaced: [{ origin: "built in", path: "(shipped with the product)" }],
  winner: { origin: "yours", path: fileIn("example-flow") },
};

const ALONE: FlowChain = {
  name: "a-home-flow",
  resolved_in: "/a-project/a-tree",
  replaced: [],
  winner: { origin: "yours", path: fileIn("a-home-flow") },
};

function row(chain: FlowChain): FlowGroup["flows"][number] {
  return { name: chain.name, note: "1 steps", dirty: false, live: null, replaces: replacesWords(chain), chain: chainWords(chain) };
}

function column(): HTMLElement {
  const groups: FlowGroup[] = [{ origin: "yours", flows: [row(REPLACING), row(ALONE)], broken: [] }];
  return render(
    <World
      native={false}
      source="engine"
      here="board"
      onGo={() => {}}
      counts={{}}
      terminals={[]}
      onMoved={() => {}}
      onTree={() => {}}
      flowGroups={groups}
      focusName={null}
      onFlow={() => {}}
      onNewFlow={() => {}}
    />,
  ).container;
}

function rowOf(container: HTMLElement, name: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>("button.rail__item")].find(
    (one) => one.querySelector(".rail__label")?.textContent === name,
  );
  expect(found, `the column drew no row for ${name}`).toBeDefined();
  return found as HTMLElement;
}

function bar(chain: FlowChain) {
  const flow = { id: chain.name, description: "", graph: { steps: [] }, inputs: {} } as unknown as FlowFile;
  return render(
    <TooltipProvider>
    <FocusBar
      name={chain.name}
      color="#888"
      flow={flow}
      bar={{ steps: 1, dirty: false, busy: false, starting: false, status: { live: false, word: "no run of this flow yet" } }}
      neverSaved={false}
      chain={chain}
      onRename={() => {}}
      onDescription={() => {}}
      onDelete={() => {}}
      onSave={() => {}}
      onRun={() => {}}
    />
    </TooltipProvider>,
  ).container;
}

describe("the column marks a flow that replaces another", () => {
  test("A ROW WHOSE FLOW REPLACES ANOTHER WEARS THE MARK, AND THE CHAIN WITH ITS DIRECTORY", () => {
    const mark = rowOf(column(), "example-flow").querySelector(".rail__replaces");
    expect(mark, "the replacing row carries no mark").not.toBeNull();
    expect(mark?.textContent).toBe(t("window.flow.replaces", { replaced: "built in" }));
    const title = mark?.getAttribute("title") ?? "";
    expect(title).toContain("/a-project/a-tree");
    expect(title).toContain("(shipped with the product)");
    expect(title).toContain(fileIn("example-flow"));
  });

  test("A ROW THAT REPLACES NOTHING WEARS NO MARK", () => {
    expect(rowOf(column(), "a-home-flow").querySelector(".rail__replaces")).toBeNull();
  });
});

describe("the flow's bar says the same", () => {
  test("THE FOCUSED FLOW SHOWS THE MARK AND THE WHOLE CHAIN", () => {
    const mark = bar(REPLACING).querySelector(".focusbar__chain");
    expect(mark, "the bar carries no mark").not.toBeNull();
    expect(mark?.textContent).toBe(replacesWords(REPLACING));
    expect(mark?.getAttribute("title")).toBe(chainWords(REPLACING));
  });

  test("A FLOW THAT REPLACES NOTHING SHOWS NO MARK IN ITS BAR", () => {
    expect(bar(ALONE).querySelector(".focusbar__chain")).toBeNull();
  });
});

describe("the chain, as it is read and said", () => {
  test("THE SHELL IS ASKED BY ONE NAME", async () => {
    const asked: string[] = [];
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (command: string) => {
          asked.push(command);
          return Promise.resolve([REPLACING]);
        },
      },
    };
    expect(await flowChains()).toEqual([REPLACING]);
    expect(asked).toEqual(["flow_chains"]);
  });

  test("THE WORDS GO LEAST SPECIFIC FIRST, AND SAY WHERE WHEN NOBODY KNOWS", () => {
    const words = chainWords(REPLACING);
    expect(words.indexOf("(replaced)")).toBeGreaterThan(-1);
    expect(words.indexOf("(replaced)")).toBeLessThan(words.indexOf("(this one runs)"));
    expect(chainWords({ ...REPLACING, resolved_in: null })).toContain(t("window.flow.chain_unknown_directory"));
    expect(replacesWords(ALONE)).toBeNull();
  });
});
