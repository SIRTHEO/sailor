// @vitest-environment jsdom
/**
 * **«NOT HERE» AND «I COULD NOT LOOK» LEAD TO DIFFERENT GESTURES**: one is an
 * install, the other a check that would not run. Merged, they send somebody to
 * install a second copy of what they already have.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { MachineScreen } from "./MachineScreen";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const SWEEP = {
  tools: [
    { id: "claude", name: "Claude Code", kind: "ai_cli", path: "/usr/local/bin/claude", version: "2.1.247",
      available: true, presence: "present", reason: "found in /usr/local/bin", descriptor: "claude" },
    { id: "gemini", name: "Gemini CLI", kind: "ai_cli", path: null, version: null,
      available: false, presence: "absent", reason: "no such file in any PATH entry", descriptor: "gemini" },
    { id: "ollama", name: "Ollama", kind: "ai_cli", path: "/opt/ollama", version: null,
      available: false, presence: "undetermined", reason: "the version probe timed out after 2s", descriptor: "ollama" },
  ],
  looked_in: ["/usr/local/bin", "/opt/homebrew/bin"],
  problems: [
    { source: "~/.config/sailor/tools.d/mine.json", about: "entry 3", reason: "missing the field «detect»" },
  ],
};

function engineAnswers(sweep: unknown = SWEEP): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: { invoke: () => Promise.resolve(sweep) },
  };
}

describe("the machine screen", () => {
  test("A TOOL NOBODY COULD CHECK DOES NOT READ AS A MISSING ONE", async () => {
    engineAnswers();
    const { container } = render(<MachineScreen native />);
    await waitFor(() => expect(screen.getByText("Claude Code")).toBeTruthy());

    const states = [...container.querySelectorAll("td[data-state]")].map((cell) => cell.getAttribute("data-state"));
    // THE CONTROL FIRST: with one state drawn, everything below would hold on
    // a screen that says nothing.
    expect(new Set(states).size, `the states arrived as ${states.join(", ")}`).toBeGreaterThan(1);
    // «absent» is hidden by default and «undetermined» is not: an unchecked
    // tool is not a tool you decided you do not have.
    expect(states).toContain("undetermined");
    expect(container.textContent).toContain("could not look");
    expect(container.textContent).not.toContain("Gemini CLI");
  });

  test("A FAULT IN THE LIST IS SHOWN APART, and before the list it damages", async () => {
    engineAnswers();
    const { container } = render(<MachineScreen native />);
    await waitFor(() => expect(container.textContent).toContain("would not read"));

    expect(container.textContent).toContain("missing the field «detect»");
    // It comes first: while it stands, everything under it was drawn from an
    // incomplete set of instructions.
    const blocks = [...container.querySelectorAll(".panel__block")];
    expect(blocks[0].textContent, "the broken line is not the first thing said").toContain("would not read");
  });

  test("WHERE IT LOOKED IS ALWAYS SAID, or the list cannot be contradicted", async () => {
    engineAnswers();
    const { container } = render(<MachineScreen native />);
    await waitFor(() => expect(container.textContent).toContain("Where it looked"));
    expect(container.textContent).toContain("/opt/homebrew/bin");
  });

  test("A SWEEP THAT FOUND NOTHING STILL SAYS WHERE IT SEARCHED", async () => {
    engineAnswers({ tools: [], looked_in: [], problems: [] });
    const { container } = render(<MachineScreen native />);
    await waitFor(() => expect(container.textContent).toContain("Where it looked"));
    // Not «no tools» in silence: an empty sweep with no directory named is
    // indistinguishable from a sweep that never ran.
    expect(container.textContent).toContain("no directory was searched");
  });

  test("A VERSION NOBODY OBTAINED IS NOT A DASH PRETENDING TO BE ONE", async () => {
    engineAnswers();
    const { container } = render(<MachineScreen native />);
    await waitFor(() => expect(screen.getByText("Ollama")).toBeTruthy());
    const row = [...container.querySelectorAll("tr")].find((tr) => tr.textContent?.includes("Ollama"));
    expect(row?.textContent).toContain("not stated");
  });
});
