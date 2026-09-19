// @vitest-environment jsdom
/**
 * **A CHAIN THAT COULD NOT BE READ IS SAID, NOT DRAWN AS «REPLACES NOTHING».**
 * `App` is imported after `window.__TAURI__` exists: `NATIVE` is read once, at
 * module load. The shell answers the flows and refuses `flow_chains`.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, test } from "vitest";
import { t } from "./i18n";

beforeAll(() => {
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  (globalThis as unknown as { DOMMatrixReadOnly: unknown }).DOMMatrixReadOnly = class {
    m22 = 1;
    constructor(_transform?: string) {}
  };
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

function aFlow(id: string) {
  return {
    state: "loaded",
    origin: "yours",
    flow: {
      id,
      description: "",
      inputs: {},
      graph: {
        steps: [
          {
            id: "say",
            deps: [],
            action: "shell_check",
            max_attempts: 1,
            when: null,
            input_schema: { type: "any" },
            output_schema: { type: "any" },
          },
        ],
      },
    },
  };
}

function pretendNativeShell(answers: Record<string, unknown>, refused: Record<string, string>) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        if (command in refused) return Promise.reject(new Error(refused[command]));
        if (!(command in answers)) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(answers[command]);
      },
    },
  };
}

describe("the chains the window could not read", () => {
  test("EVERY ROW SAYS THE CHAIN IS UNREAD, AND NONE LOOKS LIKE IT REPLACES NOTHING", async () => {
    pretendNativeShell(
      { flows: [aFlow("example-flow"), aFlow("a-home-flow")] },
      { flow_chains: "the shell could not read the flows folder" },
    );
    const { default: App } = await import("./App");
    const { container } = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /^Board/ }));

    const said = t("window.flow.chains_unread");
    await waitFor(() => {
      expect(container.querySelectorAll("button.rail__item").length).toBe(2);
      expect(container.textContent).toContain(said);
    });
    for (const row of container.querySelectorAll("button.rail__item")) {
      const mark = row.querySelector(".rail__replaces");
      expect(mark, `${row.textContent ?? ""} is drawn as if it replaced nothing`).not.toBeNull();
      expect(mark?.textContent).toBe(said);
      expect(mark?.getAttribute("title")).toContain("the shell could not read the flows folder");
    }
  });
});
