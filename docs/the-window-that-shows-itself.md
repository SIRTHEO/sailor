# The window that shows itself

**02/09/2026.** Born from a sentence of Theo's — *«the user has to be able to
see everything Sailor is, from the things it saves to how it manages them»* —
and from a second one that is its method: *«not everything ought to exist in the
foreground»*.

It is not a new design. It is the design that follows from things already
written in this repository, found again rather than invented.

## The yardstick, which was already the permanent constraint

> **Clarity for whoever is looking.** Sailor exists so that a person may see and
> control what their tools do. It holds for the look as well: **an interface
> that hides what is happening is the opposite of the product.**
>
> — `docs/decisions.md`, among the permanent constraints

Fault 30 is its sharpest violation: the canvas said «waiting» on every node of
every flow while the engine was working. **It was not hiding: it was telling a
falsehood.** The lesson that remains is that a surface which does not know a
thing has to say so, not fill the gap with a plausible value.

## The structure comes from the four surfaces, not from a list of pages

`docs/the-four-surfaces.md` gives the system four categories, and every
registered action declares one of them:

| surface | what it does | where it shows today |
|---|---|---|
| `sense` | reads the world without touching it | nowhere |
| `act` | touches the world | the board, the terminals |
| `remember` | the store, **as a source you put questions to** | almost nowhere |
| `gate` | who may do what, and where a person comes in | nowhere |

**The biggest gap is `remember`**, and the document already says it in the words
of a mandate from August:

> *«Sailor records everything that happens and never goes back to read it.»*

This is the milestone that is missing. Not one more screen: **the store becoming
interrogable by whoever is looking.**

## Eleven entries become four

Today's window has seven places in a row, all of the same weight; my first
preview had eleven. Both are wrong in the same way, and `navigation-patterns`
names it: *«mixing navigation levels in the same visual component»*. But the
real defect comes before the graphics — **it is that every capability of the
engine was asking for its own entry.**

Four entries, and each is a question a person actually asks:

| entry | the question | what it holds |
|---|---|---|
| **Board** | «what am I doing now» | the flows, the canvas, the box of steps |
| **Terminals** | «what is running» | the live terminals, the worktrees |
| **Memory** | «what happened, and what did it cost» | runs, costs, faults, quota — the interrogable store |
| **Sailor** | «what it knows about me, and what it can do» | profiles, engines, models, the home, the equipment |

The first two are `act`: where the work happens. The third is `remember`. The
fourth is the system showing itself — and it is where `sense` and `gate` will
find their place once they exist.

## What «not in the foreground» means

Three degrees, and the rule for being in each:

1. **Always visible** — the bar: where you are, what is running, what it costs.
   Three facts, and nothing else. If a run is under way you have to know it from
   anywhere.
2. **One entry away** — the four sections. Each opens on what that question
   wants, not on a menu of sub-questions.
3. **Inside the section** — all the rest. The models are not a place: they are a
   tab inside Sailor. The quota is not a place: it is a line of the bar that
   opens in Memory.

The yardstick for deciding the degree: **how many times a day**. A profile gets
changed once a week and today takes up the same width as the board.

## What «Sailor» has to show, concretely

It is the entry that today does not exist in any form, and it is the one that
answers the sentence this document is born from.

- **What it saves**: the flows and which of the three sources they come from
  (system, home, project); the store of runs; the inventory of the machine; the
  faults; the signing identities. With **the real path on disk**, because a
  piece of data whose place you do not know is a piece of data you do not
  control.
- **How it manages it**: how much room it takes; for how long; what happens when
  it grows. The plan for the life cycle of the space already exists and is
  measured: 43 GB of scratchpad, 24 GB of build directories.
- **What it can do**: the registered actions with their surface and the powers
  they demand — network, disk, processes, money, secrets. It is already the
  contract every action declares; today nobody can read it.
- **What it does it with**: engines, profiles, their access state, the models
  and the prices.

## The language

`decisions.md`, a decision of 01/09/2026 taken by Theo: *«English everywhere,
restoring the charter the project was founded with»* — identifiers, comments,
documentation, **and every message a user of the tool can see**.

The first preview was in Italian. Redone in English: it is not a preference, it
is a decision already taken that I had not read.

## What this document became on the screen, the evening of 02/09/2026

Checked against the window built the same day, on `sorgenti` from `ea2f6bf7`
to `9768998b`, by two fresh-context judges whose findings were fixed in the
last two commits. Outcome first, then what the judges left open, then what is
still Theo's.

- **Four places, not eleven.** The column drew Board / Terminals / Memory /
  Sailor, grouped «work · what happened · itself», each with a sub-rail:
  Memory had runs / ledger / spend / faults, Sailor had keeps / can do /
  profiles / models / equipment / commands, Terminals had live / projects /
  worktrees. The column counted the flows and the open terminals. **The last
  section of this document says what became of that shape.**
- **The bar speaks from anywhere, and says when it cannot.** Breadcrumbs say
  where you are, down to the ledger table open; chips say what runs, what it
  costs today (a floor when a call had no price; it opens Memory › spend),
  whether the build under the window is old, and who the command lines run
  as. A poll the engine refuses is shown in red with the reason, never read
  as «nothing running». ⌘K opens a palette that reaches any place, any flow,
  and runs one.
- **The ledger is interrogable.** Memory › ledger is a SQL box over the real
  `state.db`, read-only by construction (`PRAGMA query_only`), every table
  listed with its count, every row openable.
- **What Sailor keeps, with the real paths.** Sailor › keeps lists the home,
  every store (flows by source, the ledger, the inventory, the faults, the
  profiles, the prices, the terminals) with where it is, how many, how big,
  whether it exists yet; the binary in service, its build time and commit.
- **The terminals are a grid.** Every open terminal on screen, the focused one
  first and large, each with its own close; a line under each goes to the
  router, and a line that names a flow starts it. The pane says the bytes
  moved and what they amount to in tokens, against the ceiling the relay flow
  declares, marked as the estimate it is.
- **A run can be stopped by hand** before its next step; the step at work
  finishes, and the console says so instead of pretending.
- **The window moves into a project** from Terminals › projects; the board's
  flows follow, the terminals keep the tree they were opened in.
- **A step handed to a person is taken and closed from the window**, under
  the waiting run, through the engine's own `step open` / `step close`; the
  run resumes through the window, so the console follows it and Stop
  applies, and the answer names the root it resumed in.
- **A node nobody ran says «not run yet»**, never «waiting»: fault 30 had
  come back through the default word.
- **The empty board says where it looked**, with the real paths.
- **A dark scheme**, one at-rule the contrast engine reads into, measured on
  every scene the light one is.
- **English on every screen.** The loose-line ratchet fell from 109 to 3, and
  the measurer now reads the JSX lines it used to skip.

### What the judges left open, ranked

1. The bar still carries the board's controls (source word, flow name, Save,
   Run, the Graph tab) beside the three facts; the spec says «three facts and
   nothing else».
2. Sailor › models is the spend screen again; a catalogue with «which is in
   use» distinct from the spend does not exist.
3. Sailor › can do lists families and names, not the surface (sense / act /
   remember / gate) nor the powers each action claims.
4. Sailor › keeps has no «what happens when it grows» and no signing
   identities. «Since when» is there since `98c80fc6`: every store row
   carries its birth time from the disk.
5. The band on the board has no state, no cost per flow, no legend; the pane
   has no program+model header. The chip says «relay · 3 of 7 · at verify»
   since `98c80fc6`, and no total when the flow cannot be read back.
6. The ledger keeps no root of a run's own: a run resumed after a move into
   another project resumes in the window's root, and says so.

### What is still Theo's: the gestures

1. Open the window and walk the sections; the bar must never go silent.
2. Memory › ledger: type `select * from runs order by 1 desc limit 5`.
3. Sailor › keeps: every path listed must exist where it says.
4. Terminals: open two, type `git status` under one, watch the line route.
5. Start the relay from the board, press Stop, read the console's last line.
6. Terminals › projects: «work here» on another project; the board changes.
7. Set the machine to dark; nothing must become unreadable.

## What became of the four places, 06/09/2026

Four entries in a column was the right answer to «every capability asks for its
own entry», and the wrong answer to where the work happens. The window now
opens **on the terminals**, and they are not a destination among several: they
are the ground the stage holds until a section is asked for.

The shape today, in `desktop/src/places.ts`, is **three grounds rather than one
list**. What belongs to the tree you stand in hangs under that tree — Board,
Changes, Whiteboard, Runs. What belongs to this machine — profiles, engines,
models, quota, what Sailor keeps, what it can do — is the same wherever you
stand, and is reached by its own name through ⌘K instead of hiding two clicks
deep under a noun. `World.tsx` draws the column; there is no `Rail.tsx` any
more, and the six sections a person can stand in are named once, in `SECTIONS`.

Two things this cost, and both are recorded as faults rather than smoothed
over: a section that draws no canvas had a bar still speaking of a flow, and
the picture-taking that checks these screens was blind on the four scenes it
could not reach. **A document that names a file which no longer exists is the
same defect in prose**, which is why this section exists at all.
