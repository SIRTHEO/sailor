# Mandate: Sailor runs on every engine, and develops itself

The one prompt that starts it, to paste into a fresh session opened in this
repository:

> Your whole mandate is in `docs/2026-09-02-mandate-sailor-runs-on-every-engine.md`.
> Read it in full, then read in full the specification it names, then carry it
> out to the end.

## Why you are being asked

Theo works inside Orca and wants to work inside Sailor. Sailor already runs
flows on four command lines, keeps a ledger of every call, holds terminals in
a host, and shows itself in a window. What it does not do yet: choose an
engine by the fuel it has left, run the same flow from any seat, develop
itself under its own gate, know who is working on what, and let Orca go. The
specification says exactly which claims make each of those true.

## Your objective, whole

**Make every claim of `docs/2026-09-02-sailor-runs-on-every-engine.md` true.**

That document is your stopping condition and the only source of truth on
what «done» means. Read it in full before writing a line. Groups A to F, in
that order; inside a group you decide. A.6 is a question for Theo: ask it in
your first message, then go on with A.1 to A.5, which do not depend on it.

## How you work, and it is not how the last session worked

- **Sailor's own flows do the work where a flow exists.** `dispatch-the-work`
  splits a task across engines; `sviluppa-sailor` implements under a blind
  judge; `smista-il-lavoro` routes; `migrate-to-sailor` uninstalls. Run them
  with `sailor flow run <id>` and follow them in the window or with
  `sailor flow list`. Write code by hand only where the specification names a
  piece that no flow can produce yet (the readers of A.1, the `apply_patch`
  action, the canary). A session that hand-writes what a flow could have run
  has not used the product it builds.
- **Ask the index before you grep.** SocratiCode is indexed on this
  repository (`codebase_status` is green, 3 612 chunks): use `codebase_search`
  for «where is X decided» questions, and `codebase_impact` before touching a
  crate many depend on. Grep for exact strings only.
- **Specialised agents, not one general one.** Research goes to an agent with
  web tools; codebase questions go to Explore; plans to Plan; judgement to a
  fresh-context judge that did not write the code. Name the role in the
  prompt. Never spawn two judges on the same crate at once, and never touch
  the tree while a judge runs: judges mutate tracked files.
- **This terminal watches.** Once C.4 exists, run `watch-the-crew` at each
  beat and report what it says instead of describing what you did.

## How you verify, and it is not self-assessment

- **Who creates does not judge.** When you declare a group done, a
  fresh-context subagent that did not write that code judges it: give it the
  specification and the tree and ask, claim by claim, *yes with the proof, or
  no*. Your opinion of your work is not a check.
- **A proof counts only if it could have come out differently.** After
  writing it, put the defect back and watch it turn red. Green on a test
  written in the same breath as the code is air.
- **The absurd control goes first.** If the case that must fail passes, throw
  away every number of that pass.
- Before claiming progress, compare it with a command's output in this
  session. If a test is red, say so with its output.

## The traps, measured, not guessed

- `openpty` is denied inside the sandbox: twenty false reds. Measure outside.
- Two test batteries in parallel fight over pseudo-terminals:
  `posix_openpt: os error -6` is contention, not regression.
- `cargo test` without `--no-fail-fast` stops at the first red binary and the
  rest never run. A pipeline's exit is the last command's: write to a file and
  read the file.
- `desktop/src-tauri` is its own cargo workspace; the window's tests are
  `npx vitest run` and `npx tsc --noEmit` inside `desktop/`.
- `x.ts` and `X.tsx` are one module on this disk: contract `x.ts`, screen
  `XScreen.tsx`.
- Comment blocks over six lines (delimiters count; adjacent blocks merge) and
  dated comments are ratcheted by `comments_do_not_crowd_out_the_code`: the
  seeds are re-measured exactly and only ever lowered. `copy.test.ts` ratchets
  Italian lines on screen the same way.
- An absolute path into this machine's home directory, in a test, a product
  file or a doc, turns `nothing_from_this_machine_is_published` red: write
  `~/...`. The sentence you are reading was red the first time.
- After a change to a contract the terminal host or the hooks read, release
  the binary (`sailor release sailor`) and copy it to `~/.local/bin/sailor`,
  or the window reads an old shape.
- The session shell keeps the last `cd`: use absolute paths and `git -C`.

## Absolute constraints

- **No push to origin.** No `push`, `pull`, `fetch` followed by a reset, or
  `rebase` on `origin/*` until Theo authorises.
- **Never `git clean`.** Never `git checkout --` on a shared file without
  measuring whose work is in the diff. Measure `git diff --cached
  --diff-filter=D` before every commit: deleting a test does not fail it.
- Commit messages are written to a file and passed with `-F`; Conventional
  Commits, lowercase imperative subject, body says why, no tooling trailers.
- Everything in the repository is English: identifiers, comments, docs,
  commit messages, every sentence a user sees. Flow and step ids and
  `.flow.json` names stay as they are.
- Untracked files under `design/` belong to nobody: do not touch or commit
  them. Commit with explicit `git add <paths>`.
- Files are written with the file-editing tools, never with `sed -i` or
  heredocs.

## Autonomy

You work alone and Theo does not watch in real time. For reversible actions
that follow from this mandate, proceed without asking. Stop and ask only for a
destructive or irreversible action, a real change of scope, or something only
he can give (A.6) — and if you stop, ask and close the turn instead of closing
on a promise. Before closing a turn, look at your last paragraph: if it is a
plan, an analysis or «now I will do X», do X now with the tools. You have
context in abundance: do not stop for context reasons.

## The starting state, measured on 02/09/2026

- Branch `sorgenti` at `98c80fc6`. The window: 44 test files, 336 tests,
  green; `tsc` clean. The Rust workspace: 115 test binaries, 1 177 tests,
  green outside the sandbox; the shell's workspace 46 tests, green.
- `sailor remaining` reads Claude only: five-hour used 12 %, seven-day used
  42 %. Codex profiles `lavoro` and `prove` are not authenticated.
- `sailor workspace list` says no project has been opened yet.
- The two research reports of the same evening are in `docs/` under the
  same date: `how-others-run-on-many-engines` (read before A) and
  `memory-self-knowledge-and-specialists` (read before D and before you
  spawn any agent). The ledger says who worked so far: nineteen Claude calls
  for 37.89 $, four codex, one agy, nothing else.
- Which model to pull on ollama for B.5 and A.7: `qwen2.5-coder:7b`, the
  smallest coder model that follows a file-by-file mandate; measure it on one
  file before the sweep and write the measure in the outcome.

## What you deliver

Commits on `sorgenti`, one per piece that stands on its own. At the end, an
outcome section under the specification that marks every claim with its
result and the proof that holds it — and, where one stayed no, the measured
reason. The binary in service released from the last commit. A report to Theo
in Italian, outcome first, in whole sentences, without the jargon you built
while working, ending with the seven gestures.
