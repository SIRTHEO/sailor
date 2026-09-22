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
      invoke: (command: string) =>
        command === "workspaces"
          ? Promise.resolve(projects)
          : Promise.resolve({ name: "a-code-project", rules: ["one"], checks: {}, equipment: null }),
    },
  };
}

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

  // The refusal the window states, not the one the call throws: the two read
  // alike, and a test that takes either passes for the wrong reason.
  test("outside the shell it says so, rather than showing an empty list", async () => {
    render(<TheWindow native={false} />);
    await waitFor(() => {
      expect(screen.getByText(/no machine to ask/)).toBeDefined();
    });
  });
});
