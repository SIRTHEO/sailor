// @vitest-environment jsdom
/**
 * **THE CATALOGUE IS ASKED OF THE SHELL, AND WHAT THE SHELL REFUSES IS SAID.**
 * Through the native `invoke`: the entries come from `flow_catalogue`, the flow
 * is made by `flow_from_catalogue`, and a refusal is a line on screen.
 */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CatalogueDialog } from "./CatalogueDialog";
import type { CatalogueReading, MadeFlow } from "./startfrom";
import { t } from "./i18n";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const READING: CatalogueReading = {
  workspace: "/nowhere/a-workspace/flows",
  home: "/nowhere/a-home/flows",
  entries: [
    {
      name: "a-template",
      kind: "template",
      purpose: "Text written by the engine you name.",
      inputs: [{ name: "engine", means: "the engine that writes", required: true }],
      steps: [
        { id: "trigger", action: "trigger", deps: [] },
        { id: "the_writer", action: "external_engine", deps: ["trigger"] },
      ],
    },
    {
      name: "an-example",
      kind: "example",
      purpose: "A check stops the run.",
      inputs: [],
      teaches: { capability: "a failing command ends the run", result: "the run ends failed" },
      steps: [{ id: "refuse", action: "shell_check", deps: [] }],
    },
  ],
};

interface Call {
  command: string;
  args?: Record<string, unknown>;
}

function pretendNativeShell(answer: (call: Call) => unknown): Call[] {
  const calls: Call[] = [];
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        const value = answer({ command, args });
        if (value instanceof Error) return Promise.reject(value);
        if (value === undefined) return Promise.reject(new Error(`the fake shell has no ${command}`));
        return Promise.resolve(value);
      },
    },
  };
  return calls;
}

function ready(made: MadeFlow | Error = { flow: "x", origin: "this project", directory: "/nowhere" }) {
  return pretendNativeShell(({ command }) => (command === "flow_catalogue" ? READING : made));
}

describe("starting a flow from the catalogue", () => {
  test("THE ENTRIES COME FROM THE SHELL, WITH PURPOSE, STEPS AND INPUTS", async () => {
    const calls = ready();
    render(<CatalogueDialog onClose={() => {}} onMade={() => {}} />);

    expect(await screen.findByText("Text written by the engine you name.")).toBeTruthy();
    expect(screen.getByText("the_writer")).toBeTruthy();
    expect(screen.getByText("the engine that writes")).toBeTruthy();
    expect(calls.map((call) => call.command)).toEqual(["flow_catalogue"]);

    fireEvent.click(screen.getByRole("button", { name: /an-example/ }));
    expect(screen.getByText("a failing command ends the run")).toBeTruthy();
    expect(screen.getByText(t("window.catalogue.no_inputs"))).toBeTruthy();
  });

  test("THE FLOW IS MADE THROUGH INVOKE ONLY ONCE THE REQUIRED INPUT AND THE NAME ARE THERE", async () => {
    const madeFlows: MadeFlow[] = [];
    const calls = ready({ flow: "notes", origin: "this project", directory: "/nowhere/a-workspace/flows" });
    render(<CatalogueDialog onClose={() => {}} onMade={(made) => madeFlows.push(made)} />);
    const create = (await screen.findByRole("button", { name: t("window.catalogue.create") })) as HTMLButtonElement;

    expect(create.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText(t("window.catalogue.name")), { target: { value: "notes" } });
    expect(create.disabled, "the required engine is still blank").toBe(true);
    fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "an-engine" } });
    expect(create.disabled).toBe(false);
    fireEvent.click(create);

    await waitFor(() => expect(madeFlows).toHaveLength(1));
    const made = calls.find((call) => call.command === "flow_from_catalogue");
    expect(made?.args).toEqual({ entry: "a-template", name: "notes", inputs: { engine: "an-engine" }, place: "workspace" });
  });

  test("YOURS IS CHOSEN WHEN ASKED, AND A WORKSPACE NOT IN SIGHT CANNOT BE", async () => {
    const calls = pretendNativeShell(({ command }) =>
      command === "flow_catalogue" ? { ...READING, workspace: null } : { flow: "mine", origin: "yours", directory: "/nowhere/a-home/flows" },
    );
    render(<CatalogueDialog onClose={() => {}} onMade={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /an-example/ }));

    const here = screen.getByRole("radio", { name: new RegExp(t("window.catalogue.here")) }) as HTMLInputElement;
    const yours = screen.getByRole("radio", { name: new RegExp(t("window.catalogue.yours")) }) as HTMLInputElement;
    expect(here.disabled).toBe(true);
    expect(yours.checked).toBe(true);
    fireEvent.change(screen.getByLabelText(t("window.catalogue.name")), { target: { value: "mine" } });
    fireEvent.click(screen.getByRole("button", { name: t("window.catalogue.create") }));

    await waitFor(() => expect(calls.some((call) => call.command === "flow_from_catalogue")).toBe(true));
    expect(calls.find((call) => call.command === "flow_from_catalogue")?.args?.place).toBe("home");
  });

  test("WHAT THE SHELL REFUSES IS SAID, AND THE DIALOG STAYS", async () => {
    const madeFlows: MadeFlow[] = [];
    ready(new Error("«notes» already exists (built in)"));
    render(<CatalogueDialog onClose={() => {}} onMade={(made) => madeFlows.push(made)} />);
    fireEvent.click(await screen.findByRole("button", { name: /an-example/ }));
    fireEvent.change(screen.getByLabelText(t("window.catalogue.name")), { target: { value: "notes" } });
    fireEvent.click(screen.getByRole("button", { name: t("window.catalogue.create") }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("already exists (built in)");
    expect(madeFlows).toHaveLength(0);
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  test("A CATALOGUE THE SHELL CANNOT READ IS SAID, NOT DRAWN AS NOTHING", async () => {
    pretendNativeShell(() => new Error("the shell broke"));
    render(<CatalogueDialog onClose={() => {}} onMade={() => {}} />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("the shell broke");
  });

  test("THE SENTENCES ARE SHOWN AS THE SHELL SAID THEM, IN THE LANGUAGE IT SPEAKS", async () => {
    const italian: CatalogueReading = {
      ...READING,
      entries: [{ ...READING.entries[1], purpose: "Un controllo ferma la corsa.", teaches: { capability: "un comando che fallisce chiude la corsa", result: "la corsa finisce fallita" } }],
    };
    pretendNativeShell(({ command }) => (command === "flow_catalogue" ? italian : undefined));
    render(<CatalogueDialog onClose={() => {}} onMade={() => {}} />);

    expect(await screen.findByText("Un controllo ferma la corsa.")).toBeTruthy();
    expect(screen.getByText("un comando che fallisce chiude la corsa")).toBeTruthy();
    expect(screen.queryByText("A check stops the run.")).toBeNull();
  });
});
