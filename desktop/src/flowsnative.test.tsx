// @vitest-environment jsdom
/**
 * **THE PLACE ASKS THE SHELL, AND ASKS ONLY WHAT THE VIEW NEEDS.** Through the
 * native `invoke`: the checkout view reads where the window stands, every
 * workspace is read only when asked for, and a late answer never lands.
 */
import { afterEach, describe, expect, test } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { FlowsScreen } from "./FlowsScreen";
import type { FlowsReading } from "./flowsbyworkspace";
import { t } from "./i18n";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const SHIPPED = "(shipped with the product)";
const HOME = "/nowhere/a-home/flows";
const PLAIN = "/nowhere/a-workspace/plain";
const SLOW = "/nowhere/a-workspace/slow";

function outside(names: string[]): FlowsReading["outside"] {
  return {
    workspace: null,
    root: null,
    branch: null,
    current: false,
    flows: names.map((name) => ({ name, resolved_in: null, replaced: [], winner: { origin: "built in", path: SHIPPED }, steps: 2 })),
  };
}

const HERE: FlowsReading = {
  outside: outside(["a-shipped-flow"]),
  contexts: [{ workspace: "a-workspace", root: PLAIN, branch: "main", current: true, flows: [] }],
};

function everywhere(shipped: string): FlowsReading {
  return {
    outside: outside([shipped]),
    contexts: [
      { workspace: "a-workspace", root: PLAIN, branch: "main", current: true, flows: [] },
      { workspace: "a-workspace", root: SLOW, branch: null, current: false, flows: [], refused: { kind: "timed_out", seconds: 5 } },
    ],
  };
}

interface Deferred {
  resolve: (value: unknown) => void;
}

function pretendNativeShell(answer: (command: string) => unknown) {
  const calls: string[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        calls.push(command);
        const value = answer(command);
        if (value instanceof Promise) return value;
        if (value === undefined) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(value);
      },
    },
  };
  return calls;
}

describe("the Flows place through the native shell", () => {
  test("THE CHECKOUT VIEW READS WHERE THE WINDOW STANDS AND NEVER ASKS FOR EVERY WORKSPACE", async () => {
    const calls = pretendNativeShell((command) => (command === "flows_here" ? HERE : undefined));
    render(<FlowsScreen native />);
    await waitFor(() => expect(screen.getByText("a-shipped-flow")).toBeTruthy());
    expect(calls).toEqual(["flows_here"]);
  });

  test("every workspace is read when chosen, and a checkout that did not answer in time is said", async () => {
    const calls = pretendNativeShell((command) => {
      if (command === "flows_here") return HERE;
      if (command === "flows_by_workspace") return everywhere("a-shipped-flow");
      return undefined;
    });
    render(<FlowsScreen native />);
    await waitFor(() => expect(screen.getByText("a-shipped-flow")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));
    await waitFor(() =>
      expect(screen.getByText(t("window.flows.context_timed_out", { root: SLOW, seconds: 5 }))).toBeTruthy(),
    );
    expect(calls).toEqual(["flows_here", "flows_by_workspace"]);
  });

  test("A READING ABANDONED BY A CHANGE OF MODE NEVER OVERWRITES THE NEWER ONE", async () => {
    const pending: Deferred[] = [];
    let asked = 0;
    pretendNativeShell((command) => {
      if (command === "flows_here") return HERE;
      if (command !== "flows_by_workspace") return undefined;
      asked += 1;
      if (asked === 1) return new Promise((resolve) => pending.push({ resolve }));
      return everywhere("the-newer-answer");
    });
    render(<FlowsScreen native />);
    await waitFor(() => expect(screen.getByText("a-shipped-flow")).toBeTruthy());

    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.here") }));
    fireEvent.click(screen.getByRole("button", { name: t("window.flows.mode.all") }));
    await waitFor(() => expect(screen.getByText("the-newer-answer")).toBeTruthy());

    await act(async () => {
      pending[0].resolve(everywhere("the-stale-answer"));
      await Promise.resolve();
    });
    expect(screen.queryByText("the-stale-answer")).toBeNull();
    expect(screen.getByText("the-newer-answer")).toBeTruthy();
  });

  test("A FOLDER OF YOURS THAT COULD NOT BE READ IS SAID, NOT DRAWN AS NO FLOWS OF YOURS", async () => {
    const refused: FlowsReading = {
      outside: {
        ...outside([]),
        troubles: [{ origin: "yours", dir: HOME, why: "Permission denied (os error 13)" }],
      },
      contexts: [],
    };
    pretendNativeShell((command) => (command === "flows_here" ? refused : undefined));
    render(<FlowsScreen native />);
    const said = t("window.flows.source_unreadable", { origin: "yours", path: HOME, why: "Permission denied (os error 13)" });
    await waitFor(() => expect(screen.getByText(said)).toBeTruthy());
    expect(screen.queryByText(t("window.flows.no_flows"))).toBeNull();
  });
  test("A SOURCE THAT DID NOT ANSWER IN TIME IS SAID, AND THE SHIPPED FLOWS STILL SHOW", async () => {
    const stalled: FlowsReading = {
      outside: {
        ...outside(["a-shipped-flow"]),
        unanswered: [{ origin: "yours", dir: HOME, refused: { kind: "timed_out", seconds: 5 } }],
      },
      contexts: [{ workspace: "a-workspace", root: PLAIN, branch: null, branch_unread: true, current: true, flows: [] }],
    };
    pretendNativeShell((command) => (command === "flows_here" ? stalled : undefined));
    render(<FlowsScreen native />);
    const said = t("window.flows.source_timed_out", { origin: "yours", path: HOME, seconds: 5 });
    await waitFor(() => expect(screen.getByText(said)).toBeTruthy());
    expect(screen.getByText("a-shipped-flow")).toBeTruthy();
    expect(screen.getByText(new RegExp(t("window.flows.branch_unread")))).toBeTruthy();
    expect(screen.queryByText(t("window.flows.no_flows"))).toBeNull();
  });
});
