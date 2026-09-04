// @vitest-environment jsdom
/**
 * **THE SCREEN SHOWS THE EVIDENCE, NOT ONLY THE CLAIM.** Every line here is a
 * claim the engine backs with a sentence, and a screen that kept the claim and
 * dropped the sentence would be the screen a guess would have drawn.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ProfileList } from "./ProfileList";
import { toAdopt } from "./profiles";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const CLIS = [
  {
    id: "codex", display_name: "Codex", executable: "codex",
    native_profiles: "supported" as const,
    native_profiles_note: "`-p/--profile` layers a config file over the base one.",
    home_mechanism: "variable" as const, home_detail: "CODEX_HOME",
    home_note: "checked with `codex doctor`.",
    home_already_here: "/una/casa/.codex",
  },
  {
    id: "antigravity", display_name: "Antigravity", executable: "antigravity",
    native_profiles: "unverified" as const,
    native_profiles_note: "no such binary in PATH: the product installs as `agy`.",
    home_mechanism: "none" as const, home_detail: "",
    home_note: "the string is absent from the binary and the home follows $HOME.",
    home_already_here: "",
  },
];

const ROWS = [
  { cli_id: "codex", name: "lavoro", home_dir: "/p/codex/lavoro", active: true,
    access: "yes" as const, said: "authenticated («Logged in using ChatGPT»)" },
  { cli_id: "codex", name: "prove", home_dir: "/p/codex/prove", active: false,
    access: "not known" as const, said: "not known: nobody looked — the descriptor says no recipe" },
];

/** A shell that answers, so the screen can be measured with no Tauri around. */
function anEngineThatAnswers(): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) =>
        command === "profile_command_lines" ? Promise.resolve(CLIS)
        : command === "profiles" ? Promise.resolve(ROWS)
        : Promise.reject(new Error(`unexpected: ${command}`)),
    },
  };
}

describe("the profiles screen", () => {
  test("EVERY CLAIM ARRIVES WITH THE NOTE THAT BACKS IT", async () => {
    anEngineThatAnswers();
    const { container } = render(<ProfileList native />);

    await waitFor(() => expect(screen.getByText("Codex")).toBeTruthy());

    // THE CONTROL FIRST: with nothing drawn, every check below would pass for
    // having looked at an empty screen.
    expect(container.querySelectorAll(".panel__block").length, "no command line drawn").toBe(2);

    for (const cli of CLIS) {
      expect(container.textContent, `«${cli.display_name}» has no note on its home`)
        .toContain(cli.home_note);
      expect(container.textContent, `«${cli.display_name}» has no note on native profiles`)
        .toContain(cli.native_profiles_note);
    }
  });

  test("A VERDICT NEVER TRAVELS WITHOUT THE WORDS BEHIND IT", async () => {
    anEngineThatAnswers();
    const { container } = render(<ProfileList native />);
    await waitFor(() => expect(screen.getByText("lavoro")).toBeTruthy());

    // «not signed in» sends you to log in, «nobody could look» sends you to the
    // engine: the two must not read the same, and each keeps its sentence.
    const cells = [...container.querySelectorAll("td[data-access]")];
    expect(cells.length, "no access cell drawn").toBe(2);
    expect(new Set(cells.map((cell) => cell.getAttribute("data-access"))).size,
      "two different verdicts read as one").toBe(2);
    for (const row of ROWS) {
      expect(container.textContent, `the verdict of «${row.name}» arrived bare`).toContain(row.said);
    }
  });

  test("A HOME THAT DOES NOT MOVE IS SAID SO, not offered as a switch", async () => {
    anEngineThatAnswers();
    const { container } = render(<ProfileList native />);
    await waitFor(() => expect(screen.getByText("Antigravity")).toBeTruthy());

    const blocks = [...container.querySelectorAll(".panel__block")];
    const antigravity = blocks.find((block) => block.textContent?.includes("Antigravity"));
    expect(antigravity?.textContent, "nothing warns that profiles here change nothing")
      .toContain("start it in the same place");

    // AND THE GESTURE IS NOT OFFERED. Saying «this changes nothing» and then
    // holding out the button that makes one is the window promising something
    // the engine cannot do — the sentence would read as decoration.
    expect(
      antigravity?.textContent,
      "the invitation to make a profile stands where a profile would do nothing",
    ).not.toContain("New profile");

    // The control: it IS offered where it works, or the check above would pass
    // on a screen that offers it nowhere.
    const codex = blocks.find((block) => block.textContent?.includes("Codex"));
    expect(codex?.textContent, "the invitation is missing where it works").toContain("New profile");
  });

  test("OUTSIDE THE SHELL IT SAYS SO INSTEAD OF SHOWING AN EMPTY LIST", () => {
    const { container } = render(<ProfileList native={false} />);
    expect(container.textContent).toContain("I cannot read the profiles");
    // An empty screen and an unanswerable one must not look alike — fault 12.
    expect(container.querySelectorAll(".panel__block").length).toBe(0);
  });
});

describe("the engine is asked, not guessed at", () => {
  test("BOTH QUESTIONS GO OUT, and one command per profile is what it costs", async () => {
    const asked: string[] = [];
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (command: string) => {
          asked.push(command);
          return command === "profile_command_lines" ? Promise.resolve(CLIS) : Promise.resolve(ROWS);
        },
      },
    };
    render(<ProfileList native />);
    await vi.waitUntil(() => asked.length >= 2);
    expect(asked).toContain("profile_command_lines");
    expect(asked).toContain("profiles");
  });
});

/**
 * **A NEW PROFILE IS AN EMPTY HOME**, and every engine lit under it starts
 * logged out. The account already on the machine is offered as it is — once,
 * and never a second time on a home some profile already holds.
 */
describe("the home that is already here", () => {
  test("the screen offers it by name, and stops offering once it is taken", async () => {
    anEngineThatAnswers();
    render(<ProfileList native />);
    await waitFor(() => expect(screen.getByText("Codex")).toBeTruthy());
    expect(screen.getByRole("button", { name: /Adopt \/una\/casa\/\.codex/ })).toBeTruthy();

    const codex = CLIS[0];
    expect(toAdopt(codex, ROWS)).toBe("/una/casa/.codex");
    expect(
      toAdopt(codex, [...ROWS, { ...ROWS[0], name: "gia-presa", home_dir: "/una/casa/.codex" }]),
      "a second profile on one home is the same account under two names",
    ).toBeNull();
    expect(toAdopt(CLIS[1], ROWS), "nothing to adopt where nobody found a home").toBeNull();
  });
});
