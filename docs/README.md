# What is here

The documents a contributor needs, in the order a reader is likely to need
them: try it, understand what it is, operate it, contribute to it. A document
is named for what it says; when it was written is what `git log` says, with
the real author and the real date.

## Try

- [`walkthrough-a-queue-of-work.md`](walkthrough-a-queue-of-work.md) — a
- [`acceptance-journey.md`](acceptance-journey.md) — the steps a person walks on the installed binary before a release is called done, and the record of every walk
  project's queue drained one task at a time, `take-the-next-work` run for
  real from a seeded fixture to a parked task and back to a person.

## Understand

- [`what-sailor-can-do.md`](what-sailor-can-do.md) — the census of the engine,
  crate by crate: the twenty crates, the thirty-three actions a step can run,
  the twenty-two commands and seventy verbs of the command line, and how much of it has a door.
- [`the-window-that-shows-itself.md`](the-window-that-shows-itself.md) — why the
  window has four places and not eleven, what «not in the foreground» means, and
  what was on the screen when it was checked.
- [`the-four-surfaces.md`](the-four-surfaces.md) — **proposal, not in force**:
  the four surfaces an action could belong to (`sense`, `act`, `remember`,
  `gate`), and the rule that would follow. The page says itself why nothing
  builds against it yet.
- [`decisions.md`](decisions.md) — the memory of the choices. The permanent
  constraints at the top are the yardstick every other decision is judged by; an
  entry below binds somebody who was not in the room when it was taken. Read it
  before repairing anything, or a road already discarded gets taken again.

## Operate

- [`the-delivery-loop.md`](the-delivery-loop.md) — the ten steps from taking
  work to closing it, the same from a terminal or as a recorded flow, and
  which of them this tree already runs.
- [`the-terminal-contract.md`](the-terminal-contract.md) — the seven commands
  the bridge exposes, the two events, and who owns which files. The Rust half
  and the React half are both written against this page; whoever finds it
  differing from the code opens a fault.
- [`how-credentials-reach-the-engines.md`](how-credentials-reach-the-engines.md)
  — how a child process gets the credentials of its own profile, why parallel
  command lines with different identities work by construction, and what is
  still missing.
- [`time-is-the-last-choice.md`](time-is-the-last-choice.md) — **proposal**:
  the four levels of a life cycle, cron being the fourth, and the five things
  to decide before a time node gets written. No periodic trigger exists yet.
- [`completion-and-required-steps.md`](completion-and-required-steps.md) —
  what a completed flow does and does not establish, what a `required` step
  withholds that `decides_done` permits, and what process-boundary
  enforcement is still missing.

## Contribute

- [`faults-encountered.md`](faults-encountered.md) — the faults still open,
  one line each. The page is generated from Sailor's own fault store by
  `sailor faults render --open`; the full register, with how each fault came
  to light and **what would have stopped it**, stays in that store.

## What is not here

**The fault register is in Italian, and stays that way.** Its rows are written
by whoever met the fault and are kept in Sailor's store, which is the source
that rewrites the table: translating them in this file would tear them off it,
and the next render would put them back.

**The project's working notes are not here.** Sailor keeps them in its own
store: `sailor search <words>` finds one, `sailor notes list` shows what is
held, and `sailor notes show <slug>` reads it.
