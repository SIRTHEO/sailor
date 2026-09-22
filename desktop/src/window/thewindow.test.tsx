// @vitest-environment jsdom
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TheWindow } from "./TheWindow";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

/** The two projects the shell answers with, and what each one declares. */
function machineAnswers(projects: unknown) {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) => {
        if (command === "workspaces") return Promise.resolve(projects);
        if (command === "flows_here") return Promise.resolve(FLOWS);
        return Promise.resolve({ name: "a-code-project", rules: ["one"], checks: {}, equipment: null });
      },
    },
  };
}

/** One name the checkout decides, one it takes from the shipped set, and one
 *  whose winning file will not load. */
const FLOWS = {
  outside: { workspace: null, root: null, branch: null, current: false, flows: [
    { name: "open-the-draft-pull-request", steps: 6, resolved_in: null, replaced: [], winner: { origin: "built in", path: "/ship/open.flow.json" } },
  ] },
  contexts: [
    { workspace: "a-code-project", root: "/somewhere/a-code-project", branch: "main", current: true, flows: [
      { name: "prima-corsa", steps: 1, resolved_in: "/somewhere/a-code-project", replaced: [{ origin: "built in", path: "/ship/prima-corsa.flow.json" }], winner: { origin: "this project", path: "/somewhere/a-code-project/.sailor/prima-corsa.flow.json" } },
      { name: "notte", steps: null, broken: "unknown field `retries`", resolved_in: "/somewhere/a-code-project", replaced: [], winner: { origin: "this project", path: "/somewhere/a-code-project/.sailor/notte.flow.json" } },
    ] },
  ],
};

const TWO = [
  { root: "/somewhere/a-code-project", name: "a-code-project", first_seen: 1, last_seen: Math.floor(Date.now() / 1000), standing: "declared", current: true },
  { root: "/somewhere/gone-away", name: "gone-away", first_seen: 1, last_seen: 1, standing: "gone", current: false },
];

describe("the window, as it is mounted", () => {
  test("THE WORKSPACES ARE READ FROM THE MACHINE, not made up", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    await waitFor(() => { expect(screen.getByText("a-code-project")).toBeDefined(); });
    expect(screen.getByText("gone-away")).toBeDefined();
  });

  test("A WORKSPACE WHOSE MARKER IS GONE SAYS SO, instead of reading as old", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    await waitFor(() => { expect(screen.getByText("the marker is gone")).toBeDefined(); });
  });

  test("choosing one opens it in the field, with what it declares", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    await waitFor(() => { expect(screen.getByText("a-code-project")).toBeDefined(); });
    fireEvent.click(screen.getByText("a-code-project"));
    await waitFor(() => { expect(screen.getByText("/somewhere/a-code-project")).toBeDefined(); });
    // What the declaration does not carry is named, not left blank.
    expect(screen.getAllByText("none declared").length).toBeGreaterThan(0);
  });

  test("A LIST THAT IS NOT HERE YET SAYS WHERE IT IS, and does not draw an empty one", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    fireEvent.click(screen.getByRole("button", { name: "Data" }));
    await waitFor(() => {
      expect(screen.getByText(/store is not in this window yet/)).toBeDefined();
    });
  });

  test("THE FLOWS ARE GROUPED BY THE FILE THAT WINS, not by the file that exists", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    fireEvent.click(screen.getByRole("button", { name: "Flows" }));
    await waitFor(() => { expect(screen.getByText("prima-corsa")).toBeDefined(); });
    // The checkout's own group carries the name it decides; the shipped one
    // carries the name nothing here replaces.
    expect(screen.getByText("of this workspace")).toBeDefined();
    expect(screen.getByText("built in")).toBeDefined();
    expect(screen.getByText("open-the-draft-pull-request")).toBeDefined();
  });

  test("a flow whose file will not load says so where the count goes", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    fireEvent.click(screen.getByRole("button", { name: "Flows" }));
    await waitFor(() => { expect(screen.getByText("will not load")).toBeDefined(); });
  });

  // A NAME IS NOT A FILE: the panel lists the name, and what opens has to be
  // the file that name reaches, or the window teaches the wrong one.
  test("choosing a flow opens the file it reaches, and what it replaced", async () => {
    machineAnswers(TWO);
    render(<TheWindow native />);
    fireEvent.click(screen.getByRole("button", { name: "Flows" }));
    await waitFor(() => { expect(screen.getByText("prima-corsa")).toBeDefined(); });
    fireEvent.click(screen.getByText("prima-corsa"));
    await waitFor(() => {
      expect(screen.getByText("/somewhere/a-code-project/.sailor/prima-corsa.flow.json")).toBeDefined();
    });
    expect(screen.getByText(/\/ship\/prima-corsa\.flow\.json \(replaced\)/)).toBeDefined();
  });

  // The refusal the window states, not the one the call throws: the two read
  // alike, and a test that takes either passes for the wrong reason.
  test("outside the shell it says so, rather than showing an empty list", async () => {
    render(<TheWindow native={false} />);
    await waitFor(() => {
      expect(screen.getByText(/no machine to ask/)).toBeDefined();
    });
  });
});
