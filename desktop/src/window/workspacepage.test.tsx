// @vitest-environment jsdom
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, render, waitFor } from "@testing-library/react";
import stylesheetSource from "../styles.css?raw";
import { parseStylesheet } from "../contrast";
import type { Project } from "../workspaces";
import { WorkspacePage } from "./WorkspacePage";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const PROJECT: Project = {
  root: "/somewhere/a-code-project",
  name: "a-code-project",
  first_seen: 0,
  last_seen: 0,
  standing: "declared",
  current: false,
};

/** The case the page exists for: the declaration cannot be read, and the
 *  reason is the whole content of the answer. */
function machineRefuses(why: string) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.reject(new Error(why)) },
    event: { listen: () => Promise.resolve(() => {}) },
  };
}

describe("the facts a workspace declares", () => {
  test("A LINE THAT ANSWERS FOR THE WHOLE DECLARATION TAKES THE ROW", async () => {
    // `.window-facts` is a two-column grid whose first column is 140px wide and
    // holds labels. A `dd` with no `dt` before it is auto-placed there, so the
    // sentence saying why nothing could be read gets 140px — the one case the
    // page is drawn for is the one it draws worst.
    machineRefuses("the folder does not answer");
    render(<WorkspacePage native project={PROJECT} />);
    const said = await waitFor(() => {
      const found = document.querySelector(".window-facts__absent");
      expect(found?.textContent).toContain("the folder does not answer");
      return found;
    });
    expect(said?.classList.contains("window-facts__state"), "it would sit in the label column").toBe(true);
  });

  test("every unlabelled fact says so, and no other kind exists", async () => {
    machineRefuses("the folder does not answer");
    render(<WorkspacePage native project={PROJECT} />);
    await waitFor(() => { expect(document.querySelector(".window-facts__absent")).not.toBeNull(); });
    const children = [...(document.querySelector(".window-facts")?.children ?? [])];
    const orphans = children.filter(
      (child, at) => child.tagName === "DD" && children[at - 1]?.tagName !== "DT",
    );
    expect(orphans.every((one) => one.classList.contains("window-facts__state"))).toBe(true);
  });

  test("and the sheet is what makes it take the row", () => {
    const spans = parseStylesheet(stylesheetSource).rules.filter(
      (rule) =>
        rule.selector.includes(".window-facts__state") &&
        rule.declarations.some(([name, value]) => name === "grid-column" && value.trim() === "1 / -1"),
    );
    expect(spans.length, "the class is drawn by nothing without this rule").toBe(1);
  });
});
