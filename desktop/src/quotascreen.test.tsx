// @vitest-environment jsdom
/**
 * **A QUOTA THAT COULD NOT BE READ IS NOT A QUOTA OF ZERO.** The mistake points
 * the reassuring way, which is why it is guarded here.
 */
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { QuotaScreen } from "./QuotaScreen";

afterEach(() => {
  cleanup();
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__;
});

const NOW = 1_000_000;

const WINDOWS = [
  { engine: "claude", unit: "five_hour", spent_fraction: 0.614, resets_at: "2026-09-02T18:00:00Z", observed_at: NOW - 120 },
  { engine: "claude", unit: "thirty_day", spent_fraction: 1, resets_at: null, observed_at: NOW - 120 },
];

function engine(answers: Record<string, unknown>, fails: Record<string, string> = {}): void {
  (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    core: {
      invoke: (command: string) =>
        command in fails
          ? Promise.reject(new Error(fails[command]))
          : Promise.resolve(answers[command]),
    },
  };
}

describe("the quota screen", () => {
  test("A CHANNEL THAT DOES NOT ANSWER SAYS SO, and shows no number", async () => {
    engine({}, { quota: "the engine refused: the token has been revoked" });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(container.textContent).toContain("I could not read it"));
    // The provider's own words say what to do, so they travel whole.
    expect(container.textContent).toContain("token has been revoked");
    // AND NO BAR IS DRAWN: an empty bar reads as «nothing spent», which is the
    // one conclusion that must not be reachable from a failed reading.
    expect(container.querySelectorAll(".quota__bar").length, "a bar was drawn for a quota nobody read").toBe(0);
  });

  test("EVERY WINDOW IS SHOWN, including one this version has no name for", async () => {
    engine({ quota: { windows: WINDOWS, unreachable: [] } });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(screen.getByText("5 hours")).toBeTruthy());
    expect(container.querySelectorAll(".quota__bar").length, "a window went missing").toBe(2);
    // The unknown one appears under its own key rather than being dropped.
    expect(container.textContent).toContain("thirty day");
    // A reading carries when it was taken, or it cannot be told from yesterday's.
    expect(container.textContent).toContain("2 min ago");
  });

  /**
   * **THE QUOTA DOES NOT DRAG THE CATALOGUE BEHIND IT.** The two were one
   * component, so opening a terminal's quota fetched hundreds of models over
   * the network, and the models page led with a spend it cannot join to them.
   */
  test("AND IT ASKS FOR NOTHING BUT THE QUOTA", async () => {
    const asked: string[] = [];
    (window as unknown as { __TAURI__: unknown }).__TAURI__ = {
      core: {
        invoke: (command: string) => {
          asked.push(command);
          return Promise.resolve(command === "quota" ? { windows: WINDOWS, unreachable: [] } : undefined);
        },
      },
    };
    render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(screen.getByText("5 hours")).toBeTruthy());
    expect(asked).toEqual(["quota"]);
  });
});

/** **AN ACCOUNT THAT DID NOT ANSWER IS NOT ONE WITH ROOM.** The refusals were
 * dropped unless every account failed, so five were silent while one answered. */
describe("the accounts that did not answer", () => {
  const SOME_ANSWERED = {
    windows: WINDOWS,
    unreachable: [
      { account: "a-command-line · someone@example.test", why: "the engine refused: OAuth access token has expired. Re-authenticate to continue." },
    ],
  };

  test("ONE ACCOUNT OUT IS SHOWN WHILE ANOTHER ANSWERS", async () => {
    engine({ quota: SOME_ANSWERED });
    const { container } = render(<QuotaScreen native now={NOW} />);

    // Still not an error screen: what was read is drawn.
    await waitFor(() => expect(container.textContent).toContain("61.4"));
    expect(container.textContent, "the account that is out is not named").toContain(
      "someone@example.test",
    );
    expect(container.textContent).toContain("Re-authenticate to continue");
  });

  test("EVERY ACCOUNT OUT IS A SCREENFUL OF THEM, not an empty quota", async () => {
    engine({ quota: { windows: [], unreachable: SOME_ANSWERED.unreachable } });
    const { container } = render(<QuotaScreen native now={NOW} />);

    await waitFor(() => expect(container.textContent).toContain("someone@example.test"));
    expect(container.textContent).not.toContain("no window at all");
  });
});
