// @vitest-environment jsdom
/**
 * The accounts screen, and the contract under it. **THE TWO SIDES ARE MATCHED
 * FIELD BY FIELD**, read from the files: a rename in Rust that the window does
 * not follow otherwise reaches a person as an empty card.
 */
import { afterEach, describe, expect, test } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { AccountsScreen, tokens } from "./AccountsScreen";
import { accountName, type Account } from "./accounts";

afterEach(cleanup);

const sorgenti = import.meta.glob("../src-tauri/src/accounts.rs", { query: "?raw", import: "default", eager: true });
const contratto = import.meta.glob("./accounts.ts", { query: "?raw", import: "default", eager: true });
const rust = Object.values(sorgenti)[0] as string;
const ts = Object.values(contratto)[0] as string;

function fields(source: string, opener: string, indent: number): string[] {
  const from = source.indexOf(opener);
  if (from < 0) return [];
  const block = source.slice(from, source.indexOf("\n}", from));
  return [...block.matchAll(new RegExp(`^\\s{${indent}}(?:pub )?(\\w+)[?]?:`, "gm"))].map((m) => m[1]);
}

const AN_ACCOUNT: Account = {
  cli: "claude",
  label: "Claude Code",
  brand: "claudecode",
  profile: "work",
  active: true,
  standing: "ready",
  said: "signed in",
  calls: 12,
  spent_micros: 0,
  tokens: 4_000,
  last_call_at: 0,
  ran_out_at: null,
  windows: [
    { unit: "five_hour", word: "5 hours", used_fraction: 0.4, resets_at: "18:00", lasts_seconds: 18_000 },
    { unit: "seven_day", word: "7 days", used_fraction: 0.9, resets_at: null, lasts_seconds: 604_800 },
  ],
  quota_refused: null,
  worked: { calls: 12, sessions: 3, by_model: [{ model: "opus", input: 1_000, output: 3_000 }] },
  repair: null,
};

describe("the accounts contract", () => {
  test("EVERY FIELD IS WRITTEN ON ONE SIDE AND READ ON THE OTHER", () => {
    const pairs = [
      { what: "Row", written: fields(rust, "struct Row", 4), read: fields(ts, "interface Account", 2) },
      { what: "Window", written: fields(rust, "struct Window", 4), read: fields(ts, "interface Allowance", 2) },
      { what: "Worked", written: fields(rust, "struct Worked", 4), read: fields(ts, "interface Worked", 2) },
    ];
    for (const { what, written, read } of pairs) {
      expect(written.length, `no fields parsed out of Rust's ${what}`).toBeGreaterThan(2);
      expect(read.length, `no fields parsed out of the window's ${what}`).toBeGreaterThan(2);
      for (const field of read) {
        expect(written, `the window reads «${field}» and ${what} does not write it`).toContain(field);
      }
      for (const field of written) {
        expect(read, `${what} writes «${field}» and the window names it nowhere`).toContain(field);
      }
    }
  });
});

describe("what a card says", () => {
  test("THE TWO WINDOWS ARE NAMED APART", () => {
    render(<AccountsScreen native readings={{ state: "asked", value: [AN_ACCOUNT] }} />);
    expect(screen.getByText("5 hours")).toBeTruthy();
    expect(screen.getByText("7 days")).toBeTruthy();
    expect(screen.getByText("40%")).toBeTruthy();
    expect(screen.getByText("90%")).toBeTruthy();
  });

  test("a full window is drawn full and says so", () => {
    const out = { ...AN_ACCOUNT, standing: "ran_out" as const, windows: [{ ...AN_ACCOUNT.windows[0], used_fraction: 1 }] };
    const { container } = render(<AccountsScreen native readings={{ state: "asked", value: [out] }} />);
    expect(container.querySelector(".quota__bar[data-full]")).toBeTruthy();
    expect(screen.getByText("out of quota")).toBeTruthy();
  });

  test("AN ACCOUNT NOBODY COULD ASK ABOUT IS NOT A READY ONE", () => {
    const dark = { ...AN_ACCOUNT, standing: "unknown" as const };
    render(<AccountsScreen native readings={{ state: "asked", value: [dark] }} />);
    expect(screen.getByText("nobody could tell")).toBeTruthy();
    expect(screen.queryByText("ready")).toBeNull();
  });

  test("the engine's own words travel with the verdict", () => {
    const shut = { ...AN_ACCOUNT, standing: "shut" as const, said: "credentials not found", repair: "claude auth login" };
    render(<AccountsScreen native readings={{ state: "asked", value: [shut] }} />);
    expect(screen.getByText("credentials not found")).toBeTruthy();
    expect(screen.getByText("claude auth login")).toBeTruthy();
  });

  test("a card with no allowance read yet says that, and does not draw a zero", () => {
    const bare = { ...AN_ACCOUNT, windows: [] };
    const { container } = render(<AccountsScreen native readings={{ state: "asked", value: [bare] }} />);
    expect(screen.getByText("The allowances are not read yet.")).toBeTruthy();
    expect(container.querySelector(".quota__bar")).toBeNull();
  });

  test("a refusal to read the allowance is shown in the engine's words", () => {
    const refused = { ...AN_ACCOUNT, windows: [], quota_refused: "the provider answered 429" };
    render(<AccountsScreen native readings={{ state: "asked", value: [refused] }} />);
    expect(screen.getByText("the provider answered 429")).toBeTruthy();
  });

  test("a day with no work says nothing happened, not zero calls", () => {
    const idle = { ...AN_ACCOUNT, worked: null };
    render(<AccountsScreen native readings={{ state: "asked", value: [idle] }} />);
    expect(screen.getByText("Nothing in the last day.")).toBeTruthy();
  });

  test("outside the shell the screen says why, and shows no account", () => {
    render(<AccountsScreen native={false} />);
    expect(screen.getByText(/no engine to ask/)).toBeTruthy();
  });
});

describe("the words a card uses", () => {
  test("an account is called by the name its owner chose", () => {
    expect(accountName(AN_ACCOUNT)).toBe("work");
    expect(accountName({ ...AN_ACCOUNT, profile: null })).toBe("Claude Code");
  });

  test("tokens are rounded to what a person reads, never to zero", () => {
    expect(tokens(400)).toBe("400 tokens");
    expect(tokens(4_000)).toBe("4k tokens");
    expect(tokens(4_000_000)).toBe("4.0M tokens");
  });
});
