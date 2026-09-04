# The termination condition

**02/09/2026.** This is not a plan and not a wish list. **It is a termination
condition.** Sailor has been built for weeks without one, and a build with no
definition of done does not finish — it iterates. Every claim below is written
so that somebody who did not write it can read it, run something, and say yes
or no. When all of them are yes, the work stops and the day moves inside Sailor.

## The outcome, written the same evening

Every claim below carries an *Outcome* line: what is true now, where the
proof is, and what is still a gesture only Theo can make. Every battery named
was judged by a fresh-context judge who did not write it (one per group), and
counts only because the judge re-inserted the defect and watched it go red.
The «by hand» checks were open until 04/09/2026, when the eight gestures at
the end were performed in the installed window. What they found is written
where each claim sits, defects included. **They could not have been performed
before**: the window in service carried no page at all and opened blank —
fault 77 — so every gesture was waiting on a release that had never once
embedded the page it was releasing.

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
  `terminal_list`; a seventh, `terminal_backlog`, arrived with this run), a
  real emulator (`@xterm/xterm`), tabs, panes, liveness per
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

## What was false on 02/09, and what it is now

These are the defects the claims below are aimed at. Each was measured, not
inferred — and each was measured again on the evening of 04/09. **All eight
have fallen**, so what follows is the record of what the claims were for, not
a list of open defects.

1. ~~**A terminal dies with the window.**~~ The panes lived in a `OnceLock`
   inside the Tauri process. They live in `sailor terminal host` now, a process
   of its own: gesture A2 closed the window with ⌘Q and both tabs came back
   alive, with the `sleep` still counting under `ps -p`.
2. ~~**A comment says the opposite, and a design choice rests on it.**~~
   `desktop/src/Terminals.tsx` said «a terminal outlives the window» while it
   did not. Gesture A9 read the header against A2: it now names the reason that
   is true — the list belongs to the engine, the ptys to the host.
3. ~~**There is no scrollback.**~~ Leaving the screen and coming back showed a
   blank pane over a live process. A2 came back to both panes with their
   scrollback.
4. ~~**A profile cannot reach a terminal.**~~ The string `environment` did not
   occur in the bridge; it occurs eight times in `desktop/src-tauri/src/terminal.rs`,
   and gesture A6 read `CODEX_HOME` inside the pty as the active profile's home.
   The command line was still opening terminals with an empty environment on
   04/09 — **fault 81**, closed the same evening, with the binary itself in a
   real pty as the test.
5. ~~**A program cannot be given arguments.**~~ `Opening.args` was declared and
   never passed. `Terminals.tsx:290` passes it.
6. ~~**A tab cannot be identified.**~~ `Summary` carried no tty. It carries
   `device`, and with it `moved`, `estimated_tokens`, `program` and the profile
   in force.
7. ~~**The workspace is a path you type by hand.**~~ It is a select built by
   `placesOf` from the projects and the worktrees — and since **fault 79** each
   row carries what distinguishes it, because six of them shared one label.
8. ~~**That screen is still in Italian.**~~ None of the three sentences is
   there any more.

---

# The claims

## A. The terminals — without these the day does not start

**A1. A session survives leaving its screen.** Going to Flows and back to
Terminals shows the pane exactly as it was, including what the program printed
while away.
*Check: by hand, and a battery test that mounts, unmounts and remounts the
screen and asserts the earlier output is on the pane.*
*Outcome: yes, both.* By hand on 04/09: twelve ticks started, the Board opened
mid-loop, and coming back the pane held all twelve and the prompt after them.
`App.tsx` keeps the screen mounted
behind the other places, and a pane that comes back draws the host's backlog
first. Tests `THE SCREEN STAYS MOUNTED BEHIND THE OTHER PLACES` and `LEAVING
THE SCREEN AND COMING BACK SHOWS WHAT WAS PRINTED MEANWHILE` in
`desktop/src/terminal.test.tsx`; the judge put the conditional mount back and
dropped the backlog write, and each turned its test red.

**A2. A session survives closing the window.** Quitting Sailor and reopening it
finds the terminals still open, still running, with their output.
*Check: by hand — open a terminal, run something long, quit the app, reopen,
the tab is there and the program is alive. Plus a battery test that the pty is
not owned by the window process.*
*Outcome: yes, both.* By hand on 04/09: two tabs open — one running an agent,
one running `sleep 600` — quit with ⌘Q, reopened. Both tabs came back ALIVE
with their scrollback, and `ps -p` found the sleep still counting.
The ptys belong to `sailor terminal
host`, a process of its own (`setsid`, SIGHUP ignored) that the window starts
when none answers and talks to over a socket under the ledger directory.
`crates/sailor/tests/a_terminal_outlives_the_window.rs` runs the real binary:
a client opens a shell, leaves, a new client finds it alive with its backlog and
types more; the absurd control kills the host and watches the shell die. The
judge emptied the backlog and the test went red. The gesture needs the binary
in service to have the `host` form, which is D1.

**A3. What a terminal already printed is shown when a pane attaches to it.**
A terminal opened five minutes ago and looked at now shows what happened in
between, not a blank screen.
*Check: battery — the engine keeps a bounded backlog, the bridge serves it on
attach, and a test asserts a pane attached late receives it.*
*Outcome: yes.* The host keeps 512 KB per terminal with absolute offsets
(`crates/terminal/src/host.rs`), `terminal_backlog` serves it, and every live
event carries its offset so the pane writes each byte once.
`what_was_printed_before_anyone_looked_is_served_to_whoever_attaches_late` in
`crates/terminal/tests/a_terminal_is_held_by_the_host.rs`, and `A PANE ATTACHED
LATE SHOWS THE BACKLOG FIRST` in the window; the judge zeroed the backlog and
dropped the offset filter, red both times.

**A4. A terminal is opened by choosing a workspace, not by typing a path.**
The known workspaces and worktrees are offered; a path can still be typed.
*Check: by hand, plus a battery test that the offered list is the engine's
answer and not a list kept in the screen.*
*Outcome: yes as written, and the gesture found a defect the claim does not
cover.* By hand on 04/09: the select listed the projects, then the worktrees
each with its branch, and «…another path» last. But six of the project rows
carried **the same label** and a seventh repeated it further down: the claim
asks what is listed, not whether a person can tell two entries apart. Fault 79.
The select is built from `projects()`
and `listTrees()` only, with «…another path» keeping a typed path. `THE
OFFERED PLACES ARE THE ENGINE'S ANSWER` in `desktop/src/terminal.test.tsx`; the
judge seeded the list with one place of its own and three tests went red.

**A5. A terminal is opened by choosing what runs in it, with arguments.**
`claude --resume` starts `claude` with `--resume`, not a binary of that name.
*Check: battery — `args` crosses the bridge, asserted on both sides.*
*Outcome: yes, on three sides.* The window splits the line and sends `program`
plus `args` (`WHAT TO START CROSSES THE BRIDGE AS A PROGRAM AND ITS
ARGUMENTS`); the host opens them as arguments and refuses a joined name
(`arguments_cross_the_wire_as_arguments_and_not_as_part_of_the_name`); the
judge found the bridge in between unproven, so `what_crosses` in
`desktop/src-tauri/src/terminal.rs` now has its own test.

**A6. A terminal opened from the window runs under the active profile.**
`CLAUDE_CONFIG_DIR` / `CODEX_HOME` are in the environment of the process
inside the pty.
*Check: battery — `Opening.environment` is set from the profile store and a
test reads the child's environment. And by hand: open a terminal on the `prove`
profile and confirm the agent inside it is that profile's.*
*Outcome: yes, both.* By hand on 04/09, in a terminal opened from the window:
`CODEX_HOME` was the active `prove` profile's home, the same path
`sailor profiles list` prints. `CLAUDE_CONFIG_DIR` was **empty**, and that is
right rather than missing: no profile is declared for that command line, and
nothing was invented to fill the variable.
`profiles::active_environment` turns
the active profiles into `CLAUDE_CONFIG_DIR` / `CODEX_HOME`
(`the_active_profiles_become_the_variables_a_terminal_opens_with`), the bridge
reads the store at every open and passes them, and
`the_environment_given_at_opening_reaches_the_program_inside` reads the child's
environment through the host. The judge emptied each end in turn; red each time.

**A7. A tab says which session it is.** Every tab carries the tty, and the tty
is the anchor — not a product name, not a title guessed from output.
*Check: battery — `Summary` carries the device, the contract test on both sides
covers it, and the screen shows it.*
*Outcome: yes.* `Summary.device` is the short tty name, and
`the_device_in_the_list_is_the_one_the_program_inside_reports` runs `tty`
inside the terminal to check it; the contract test lists the field on both
sides; every tab and pane header shows it (`the list comes from terminal_list,
and every tab carries its tty and its word`). The judge renamed the field and
showed the workspace name instead: red on both sides.

**A8. Every sentence on that screen is in English.**
*Check: battery — the existing language gate extended to `Terminals.tsx`.*
*Outcome: yes, after the judge.* The first gate at zero reused the ratchet's
word list and was blind to «Spazio di lavoro» and «Cosa avviare», the very
sentences of defect 8: the judge put them back and the gate stayed green. The
gate now reads the five files of that screen with the words it used to say,
and both sentences turn it red (`EVERY SENTENCE ON THE TERMINALS SCREEN IS IN
ENGLISH` in `desktop/src/copy.test.ts`).

**A9. The comment at `Terminals.tsx:5` states what is true.** After A2 it may
become true as written; until then it says what actually holds.
*Check: by hand, reading it against the measurement in defect 1 above.*
*Outcome: yes by reading, Theo's reading still due.* The comment now says the
terminals are held by `sailor terminal host`, a process that outlives the
window, and that the screen keeps no copy of the list because the host's list
is the only one. The judge read it against the bridge, which holds no pane,
and against the outlives test, and found it true.

## B. Handing over the work — the relay

**B1. A terminal opened by the window has a letterbox.** It registers its tty
and its count the same way `sailor terminal` does.
*Check: battery — `session.rs::open` opens the inbox, and a test asserts a
window-opened terminal appears to `terminal list` with its letterbox.*
*Outcome: yes.* `Terminals::open` registers the letterbox and the count under
the tty when the host gives it a mailroom, and withdraws both when the
terminal ends. `a_terminal_the_host_opened_has_a_letterbox_and_a_count_under_its_tty`,
and the outlives test polls `sailor terminal list` until the window's terminal
is there with its bytes. The judge took the mailroom away, keyed the socket on
the id, and dropped the tally write: red three times.

**B2. A flow can write into a terminal that the window opened.** The step that
hands work to the agent already alive in a terminal reaches it.
*Check: battery — `a_line_typed_from_outside_reaches_the_program` extended to a
terminal born through the bridge. Plus by hand, once, with a real agent.*
*Outcome: yes, both, and the flow half needs a word.* By hand on 04/09, with a
real agent alive in a window-opened terminal: `sailor flow run
passa-il-testimone <tty>` completed green **without typing anything** — the
terminal was under the ceiling, so the `when` skipped the whole chain and left
the measurement behind, which is the flow's declared behaviour and not a
failure. The typing half was then driven through the same power the skipped
step uses, `sailor terminal press`: the line appeared in the agent's prompt,
was submitted, and the agent answered. So the claim holds, and what a run of
that flow proves depends on how full the terminal was when it ran.
The terminal in the test is born
through the engine's `Terminals::open` with a mailroom, which is exactly what
the host does; the judge also drove the relay's own `type_into_terminal` node
against a host-owned terminal and read the word back from its backlog. With the
letterbox thread dropping its bytes, both went red. One thing the judge noted:
the relay reports «went» when the letterbox accepted the bytes, not when the
program read them.
**And on the evening of 04/09 the chain past the ceiling ran whole, for the
first time.** A terminal Sailor owns was filled to 1,500,000 bytes — over the
500,000-token ceiling the step declares — and `sailor flow run
passa-il-testimone ttys009` took every step: the measure said past the ceiling,
the request for the mandate was typed into the terminal, the collection
answered `not_yet` because nobody had left one, and after a mandate was left
through `sailor terminal mandate` the resumed run reset the session (`/clear`)
and handed the mandate back. All three lines were read back from the
terminal's own output, in order, so «went» was checked against what the
program actually received. What is still unobserved is only the agent's half:
a real agent deciding to write the mandate when asked.

**B3. Sailor says when an agent is about to need the baton**, from the count it
already keeps, rather than from a guess.
*Check: measured — the count is read from the tally, and the screen shows the
same number the engine reports.*
*Outcome: yes on the check as written.* One tally counts what the terminal
shows and what is typed into it; `Summary.moved`, the `.seen` file, `sailor
terminal list` and the relay's `measure_terminal` all read it, and the judge saw
the same 59 bytes from all four. The outlives test asserts the list's number is
the row's number once the shell is quiet, and the pane shows `moved`. The judge
keeps two reservations: the window shows bytes and no ceiling or verdict, which
only `terminal list --ceiling` and the relay say; and «about to need» is in
fact «past the ceiling».

## C. Seeing what was done

**C1. What an agent changed can be read inside Sailor.** A read-only diff of the
working tree of a workspace: which files, and what changed in them.
*Check: by hand, on a real change, plus a battery test that the diff shown is
`git`'s answer and not one computed here.*
*Outcome: yes, both.* By hand on 04/09, with one uncommitted line in a file:
the panel showed the same hunk as `git diff HEAD` in that terminal — same
header, same index hashes, same single added line.
`workspace::changes` returns the files
of `git status --porcelain -z` and the text of `git diff HEAD`, byte for byte;
`crates/workspace/tests/the_diff_shown_is_gits_answer.rs` compares against git
run by the test, with a clean tree, a staged file and a path with a space and an
accent. The judge replaced the diff with one computed in Sailor, dropped the
untracked files, and fed a stale diff to a clean tree: red each time. The judge
also found the line form quoting such paths, and that is fixed. The screen is
reached from a terminal tab, under «what changed in …».

**C2. A file can be opened in the editor Theo already uses**, from the window.
*Check: by hand.*
*Outcome: the hand-off works, and the claim is not met.* By hand on 04/09, with
neither variable set: pressing it did open the file, in the application the
system associates with that file type — a **read-only preview**, not an editor.
So the gesture succeeds and the claim, which names an editor, does not hold.
Fault 80: the last resort is «whatever opens this kind of file», which says
nothing about editing.
Each listed file has «open in the
editor», which hands its absolute path to `SAILOR_EDITOR`, else `VISUAL`, else
the system opener (`desktop/src-tauri/src/changes.rs`). The judge flipped the
precedence and the test went red, and ran a fake editor to see the path arrive
whole, spaces included. The variables are read from the window's own
environment: a bundle launched from the Dock has neither, and then the file
association decides.

## D. Coming back tomorrow

**D1. The binary in service is the one built from these sources.** No `ui` that
no longer exists, and every command the sources declare is present.
*Check: measured — `sailor` with no arguments, compared against the command
registry.*
*Outcome: yes, measured.* `sailor release sailor` cloned HEAD `5742da24`, ran
the whole suite on it, built, and put the binary in service under
`~/.config/sailor/bin`; the same file was then copied to
`~/.local/bin/sailor`, the path the hooks name. `sailor` with no
arguments lists fourteen commands, `terminal` among them with the `host` form,
and no `ui`. The binary in service before this run was built at 13:52 and had
no `host`, which is why the window could not open a terminal on this machine
until now.

**D2. A flow that is due is run by something.** Not a cron script outside the
system: the order of preference already decided (constraint → event → deadline
at read time → cron as a safety net) holds.
*Check: measured — a flow with a schedule runs without anybody typing a
command.*
*Outcome: yes, measured.* The window beats (`desktop/src-tauri/src/beat.rs`):
a thread judges every known flow once a minute, and reading the flows judges
them too, with the same `flow::is_due` the command line uses; a flow still
running is not started on top of itself, and every beat reports what it held
and why. Measured twice on a scratch home with one flow due every five
seconds, the second time by the judge on a window built from HEAD: the window
ran for thirteen seconds with nobody typing, its stdout said `ran` at the first
beat and again six seconds later (the second from a beat triggered by the
page reading its flows), and the ledger held two complete runs with
`started_by = window · schedule`. The absurd control, the same flow with its
schedule removed, started nothing in the same thirteen seconds. Cron outside
stays as the net. One limit the judge named: «still running» is known from
this window's own runs, not from the ledger, so a run left open by a window
that was closed does not hold the next window's beat.

**D3. The window is installed the way the engine is.** Not built and launched
from inside `target/`, which a `cargo clean` empties, but put into service from
a commit, with that commit written down beside it.
*Check: measured — `sailor release window`, and the stamp names the commit.*
*Outcome: yes, measured.* The window is a target of `sailor release` like
`sailor` is, and the three things that made it different are declared rather
than guessed: `manifest_rel`, because the shell is a workspace of its own that
neither the root's build nor the root's suite enters — so a release now runs one
suite per manifest the target is made of; `page_rel`, because the shell embeds
the page at compile time, so it is built **inside the clone** before the shell,
and the page's own gate runs inside that build; and `parts_of`, so what stays
out of service and what the stamp names are read over the directories the target
is really made of. The page's modules are cloned from the working tree with
`cp -Rc` when the two `package-lock.json` are the same bytes — three seconds for
300 MB instead of an `npm ci`; a symlink was tried first and does not work, and
that is a measurement, not a preference: the page's builder refuses a file from
outside the tree it was pointed at, and the release stopped there.

---

# The gestures, performed

The eight were done on 04/09/2026 in the installed window, one after another,
in about half an hour including the repair that made them possible. Six held.
Two held as written and failed the thing the claim was really about, and both
are now in the ledger.

| | gesture | what happened |
|---|---|---|
| **A1** | ticks, leave to the Board, come back | ✅ all twelve on the pane, prompt after them |
| **A2** | agent and `sleep 600`, ⌘Q, reopen | ✅ both tabs back ALIVE with their scrollback; `ps -p` found the sleep counting |
| **A4** | the workspace select | ⚠️ lists what it claims — and six rows share one label. **Fault 79**, closed: the label carries what distinguishes it |
| **A6** | the profile's variable inside the pty | ✅ `CODEX_HOME` is `prove`'s home; the other variable empty because no profile declares it |
| **A6b** | the same variable inside `sailor terminal run` | ⚠️ empty: the command opened terminals under no profile at all, where the window opened them under the one in force. **Fault 81**, closed with an end-to-end test |
| **A9** | read `Terminals.tsx` against A2 | ✅ the header names the reason A2 works: the list belongs to the engine, the ptys to the host |
| **B2** | a line into a live agent | ✅ it lands, is submitted, and is answered. The flow itself skipped: the terminal was under the ceiling |
| **C1** | «what changed in …» against `git diff HEAD` | ✅ same hunk, same index hashes, same line |
| **C2** | «open in the editor» | ⚠️ it opens — a read-only preview, not an editor. **Fault 80**, closed: the button names who will get the file before the press |

Three things were learned that no claim asks about, and all three are in
`docs/guasti-incontrati.md`:

- **Fault 77, which was blocking every one of them.** The window in service
  carried no page and opened blank, because the release built the shell without
  the feature that embeds it. Every shell that release had ever built was
  empty. The release now declares the feature and, before replacing anything,
  reads the binary it just built looking for the page.
- **Fault 78.** The pane says «every key goes to the process as it is, Enter
  included», and a Control combination does not. A shell left on a continuation
  prompt cannot be interrupted from the window — only closed.
- The «what to start» field takes a program and its arguments, which is claim
  A5 and correct; a shell line typed into it fails with the operating system's
  own «no such file or directory», in Italian, saying nothing about what it
  expected. The Italian half is already listed above.

# What the judges found and this run left as it is

Each item is outside the claims, or inside them but beyond the check as
written. They are listed so that nobody rediscovers them.

- **Nothing drives the Tauri bridge end to end** against a live host:
  `terminal_open` and `terminal_list` are proven at the window's end, at the
  host's end and in the pure function between, but not as one round trip.
- **The window shows bytes and no verdict.** The ceiling and the «past it»
  verdict live in `sailor terminal list --ceiling` and in the relay's
  `measure_terminal`; the pane shows the count only.
- **The relay's «went» means the letterbox took the bytes**, not that the
  program read them.
- **The diff is reachable only through a terminal tab** on that workspace, and
  `SAILOR_EDITOR` is documented in `changes.rs` alone.
- **A test that opens a pty leaves its scratch directories** under the system
  temp directory even when green; two hosts left by red runs of the outlives
  test were found alive and killed, and that test now kills its host whichever
  way it ends.
- **The beat's «still running» guard is this window's memory**, not the
  ledger's: a run left open by a closed window does not hold the next one.
- **Italian still leaks where no claim looks**: the line the window prints at
  every run start, the answer to an unknown command, the error strings of
  `run.rs`, and a comment in `desktop/src-tauri/src/main.rs` that still
  describes `sailor ui` as if it existed.
- **Fault 68**, recorded by Theo's reading during this run: functions and tests
  that exist only because the tree is in Italian. In `docs/guasti-incontrati.md`.

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
- **The 13 open faults.** None of them stands on the road to the day inside.
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
