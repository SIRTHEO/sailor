# The MVP that ends Orca

**02/09/2026.** This is not a plan and not a wish list. **It is a termination
condition.** Sailor has been built for weeks without one, and a build with no
definition of done does not finish — it iterates. Every claim below is written
so that somebody who did not write it can read it, run something, and say yes
or no. When all of them are yes, the work stops and Theo moves off Orca.

## The measure

**One working day of Theo's, inside Sailor, without leaving it.** Not «Sailor
does things» — it already does many. The day is the oracle: opening a project,
running an agent in it, watching what it does, handing it work, judging what it
changed, and doing it again tomorrow.

## How to read a claim

Each claim is one sentence that is true or false, followed by **how it is
checked**. Three kinds of check appear:

| | |
|---|---|
| **battery** | a test in the repository decides it, named here |
| **by hand** | somebody opens the window and does the gesture, once |
| **measured** | a command prints a number, quoted here with the command |

A claim with no check is not a claim. If one turns out not to be checkable as
written, rewrite the claim — do not soften the check.

## What is already true, measured today

Written so the run does not rebuild it. Everything in this section was verified
on 02/09/2026 by reading the code and running commands, not recalled.

- **The terminal engine is finished**: `crates/terminal`, 2,304 lines. It opens
  a real pty, writes to it, delivers output *as it comes*, resizes, closes, and
  lists what is open and where.
- **The window already has terminals.** Six Tauri commands (`terminal_open`,
  `terminal_submit`, `terminal_press`, `terminal_resize`, `terminal_close`,
  `terminal_list`), a real emulator (`@xterm/xterm`), tabs, panes, liveness per
  tab, and an output channel that says when it did not attach. The document of
  01/09 called this «the real blocker» and it is no longer true — that document
  is stale on its own first point, and this one supersedes it.
- **Worktrees reached the window**: `worktree_list`, `worktree_create`,
  `worktree_remove` are registered.
- **The relay is built and unused.** `crates/terminal`'s `inbox`, `tally`,
  `mandate` and `bridge` are four complete, tested modules. Their only callers
  are `crates/sailor/src/terminal_cmd.rs` and `crates/relay`. **Zero references
  from `desktop/`** — verified by grep across `desktop/src` and
  `desktop/src-tauri/src`. Whatever the run needs here, it wires; it does not
  write.
- **A step can now answer «not yet, ask me on the next beat».** Fault 62 is
  closed: `take_mandate` no longer parks on `Waiting` forever, and a periodic
  source declares its missed run and its limit. Landed on the trunk today as a
  fast-forward, `6261b3bb`, 36 files, no deletions. This moves the ground under
  two claims below — **B** is built on the third outcome, not on the parking
  one, and **D2** starts from a beat that exists rather than from nothing.

## What is false today, measured

These are the defects the claims below are aimed at. Each was measured, not
inferred.

1. **A terminal dies with the window.** The panes live in an `OnceLock` inside
   the Tauri process (`desktop/src-tauri/src/terminal.rs:55-58`). Close the
   window and every session inside it is gone.
2. **A comment says the opposite, and a design choice rests on it.**
   `desktop/src/Terminals.tsx:5` states «A terminal outlives the window —
   closing it does not kill the session inside». That is false. The choice it
   justifies (keep no local copy of the list) is right for a different reason.
3. **There is no scrollback.** Output is delivered live and kept nowhere. Leave
   the Terminals screen and come back: the component unmounted
   (`App.tsx:1245` renders it behind `place === "terminals"`), the emulator was
   destroyed, and the pane is blank while the process is alive and talking.
4. **A profile cannot reach a terminal.** `Opening.environment` is public on the
   engine and the bridge never sets it — the string `environment` does not occur
   in `desktop/src-tauri/src/terminal.rs` or `desktop/src/terminal.ts`. Neither
   `CLAUDE_CONFIG_DIR` nor `CODEX_HOME` gets through.
5. **A program cannot be given arguments.** `Opening.args` is declared in
   `desktop/src/terminal.ts:82` and `Terminals.tsx` never passes it. Typing
   `claude --resume` in «what to start» looks for a binary named, literally,
   `claude --resume`.
6. **A tab cannot be identified.** `Summary` carries `id`, `workspace_root`,
   `workspace_name`, `alive`, `process_id` — and no tty, though `Pty::device()`
   exists at `crates/terminal/src/pty.rs:201`. The window cannot say which tab
   is which session.
7. **The workspace is a path you type by hand.** The «workspace» field is free
   text, while `workspaces` and `worktree_list` already answer the question.
8. **That screen is still in Italian** («Spazio di lavoro», «Cosa avviare»,
   «apro…»), against the decision of 01/09.

---

# The claims

## A. The terminals — without these the day does not start

**A1. A session survives leaving its screen.** Going to Flows and back to
Terminals shows the pane exactly as it was, including what the program printed
while away.
*Check: by hand, and a battery test that mounts, unmounts and remounts the
screen and asserts the earlier output is on the pane.*

**A2. A session survives closing the window.** Quitting Sailor and reopening it
finds the terminals still open, still running, with their output.
*Check: by hand — open a terminal, run something long, quit the app, reopen,
the tab is there and the program is alive. Plus a battery test that the pty is
not owned by the window process.*

**A3. What a terminal already printed is shown when a pane attaches to it.**
A terminal opened five minutes ago and looked at now shows what happened in
between, not a blank screen.
*Check: battery — the engine keeps a bounded backlog, the bridge serves it on
attach, and a test asserts a pane attached late receives it.*

**A4. A terminal is opened by choosing a workspace, not by typing a path.**
The known workspaces and worktrees are offered; a path can still be typed.
*Check: by hand, plus a battery test that the offered list is the engine's
answer and not a list kept in the screen.*

**A5. A terminal is opened by choosing what runs in it, with arguments.**
`claude --resume` starts `claude` with `--resume`, not a binary of that name.
*Check: battery — `args` crosses the bridge, asserted on both sides.*

**A6. A terminal opened from the window runs under the active profile.**
`CLAUDE_CONFIG_DIR` / `CODEX_HOME` are in the environment of the process
inside the pty.
*Check: battery — `Opening.environment` is set from the profile store and a
test reads the child's environment. And by hand: open a terminal on the `prove`
profile and confirm the agent inside it is that profile's.*

**A7. A tab says which session it is.** Every tab carries the tty, and the tty
is the anchor — not a product name, not a title guessed from output.
*Check: battery — `Summary` carries the device, the contract test on both sides
covers it, and the screen shows it.*

**A8. Every sentence on that screen is in English.**
*Check: battery — the existing language gate extended to `Terminals.tsx`.*

**A9. The comment at `Terminals.tsx:5` states what is true.** After A2 it may
become true as written; until then it says what actually holds.
*Check: by hand, reading it against the measurement in defect 1 above.*

## B. Handing over the work — the relay

**B1. A terminal opened by the window has a letterbox.** It registers its tty
and its count the same way `sailor terminal` does.
*Check: battery — `session.rs::open` opens the inbox, and a test asserts a
window-opened terminal appears to `terminal list` with its letterbox.*

**B2. A flow can write into a terminal that the window opened.** The step that
hands work to the agent already alive in a terminal reaches it.
*Check: battery — `a_line_typed_from_outside_reaches_the_program` extended to a
terminal born through the bridge. Plus by hand, once, with a real agent.*

**B3. Sailor says when an agent is about to need the baton**, from the count it
already keeps, rather than from a guess.
*Check: measured — the count is read from the tally, and the screen shows the
same number the engine reports.*

## C. Seeing what was done

**C1. What an agent changed can be read inside Sailor.** A read-only diff of the
working tree of a workspace: which files, and what changed in them.
*Check: by hand, on a real change, plus a battery test that the diff shown is
`git`'s answer and not one computed here.*

**C2. A file can be opened in the editor Theo already uses**, from the window.
*Check: by hand.*

## D. Coming back tomorrow

**D1. The binary in service is the one built from these sources.** No `ui` that
no longer exists, and every command the sources declare is present.
*Check: measured — `sailor` with no arguments, compared against the command
registry.*

**D2. A flow that is due is run by something.** Not a cron script outside the
system: the order of preference already decided (constraint → event → deadline
at read time → cron as a safety net) holds.
*Check: measured — a flow with a schedule runs without anybody typing a
command.*

---

# Out of scope, deliberately

Anything here that gets built during this run is scope creep, and scope creep is
the thing this document exists to stop. If one of these turns out to block a
claim above, say so and stop — do not quietly bring it in.

- **A browser inside the window.** It is the oracle for interface work, and it
  does not block the move.
- **Search across everything Sailor keeps.**
- **Sharing flows** — without a refusal to publish a secret, that is a data leak
  shaped like a feature.
- **The Bazel model of a step's powers**, decided 29/08 and not begun: it costs
  a rewrite of the existing flows.
- **An editor.** C1 and C2 are a diff and a hand-off, not an editing surface.
- **The 13 open faults.** None of them stands on the road out of Orca.
- **Tidying, renaming, and refactoring** anything the claims do not touch. A
  file that is Italian, over-commented, or ugly, and that no claim names, stays
  as it is.
- **Pushing to `origin`.** Frozen until Theo authorises it, this run included.

---

# How the run checks itself

**Every claim is verified by somebody who did not write it.** The house rule
already says chi crea non giudica; here it is the run's own interval. After each
group (A, B, C, D) a fresh-context judge is given this document and the tree,
and answers per claim: *yes, with the evidence — or no.* Self-assessment does
not count as a check, and neither does a green battery on a test written in the
same breath as the code.

**A red battery that was red before this run is not this run's result.** One
known trap, measured today: `a_terminal_lives_in_its_workspace` can fail with
`posix_openpt: os error -6` when two batteries run at once. That is pty
contention on this machine, not a regression. Run it alone before believing it.

**Progress is claimed against tool output, never against intent.** A claim moves
to yes when a command was run and its output says so.

# What this document may have wrong

It is written from the code and from measurements taken today, and from one
working day that has not been had yet. The claim most likely to be wrong is
**A2**: making a terminal outlive the window may be a bigger piece than one line
of this document suggests — the engine's own comment says the day terminals
outlive their opener, the ledger registers them, not the in-memory list. If A2
turns out to be a cantiere of its own, it is still a claim: it does not get
dropped, it gets reported as the reason the condition is not yet met.

The second most likely to be wrong is that this list is **too short**. It was
written by asking what a day needs, not by walking through one. Whoever first
spends a day inside Sailor should add what they had to leave for.
