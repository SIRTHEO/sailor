# What is here

The documents a contributor needs: what each surface promises, the decisions
that do not reopen, and the register of every fault found and how it was fixed.
A document is named for what it says; when it was written is what `git log`
says, with the real author and the real date.

## The rules the work is judged by

- [`decisions.md`](decisions.md) — the memory of the choices. The permanent
  constraints at the top are the yardstick every other decision is judged by; an
  entry below binds somebody who was not in the room when it was taken. Read it
  before repairing anything, or a road already discarded gets taken again.
- [`faults-encountered.md`](faults-encountered.md) — every fault that really
  happened, with how it came to light and **what would have stopped it**. The
  table is generated from Sailor's own fault store by
  `sailor faults render`, and `sailor faults check` compares the two.

## What the system is, and what it exposes

- [`the-four-surfaces.md`](the-four-surfaces.md) — the four surfaces an action
  can belong to (`sense`, `act`, `remember`, `gate`), and the rule that follows:
  if an orchestration calls for new code, a power is missing, not a flow.
  Declared and not yet in force — the page says so itself.
- [`what-sailor-can-do.md`](what-sailor-can-do.md) — the census of the engine,
  crate by crate: the nineteen crates, the twenty-three actions a step can run,
  the forty-nine verbs of the command line, and how much of it has a door.
- [`the-window-that-shows-itself.md`](the-window-that-shows-itself.md) — why the
  window has four places and not eleven, what «not in the foreground» means, and
  what was on the screen when it was checked.

## The contracts two halves are written against

- [`the-terminal-contract.md`](the-terminal-contract.md) — the seven commands
  the bridge exposes, the two events, and who owns which files. The Rust half
  and the React half are both written against this page; whoever finds it
  differing from the code opens a fault.
- [`how-credentials-reach-the-engines.md`](how-credentials-reach-the-engines.md)
  — how a child process gets the credentials of its own profile, why parallel
  command lines with different identities work by construction, and what is
  still missing.
- [`time-is-the-last-choice.md`](time-is-the-last-choice.md) — the four levels
  of a life cycle, cron being the fourth, and the five things to decide before a
  time node gets written.

## What is not here

**The fault register is in Italian, and stays that way.** Its rows are written
by whoever met the fault and are kept in Sailor's store, which is the source
that rewrites the table: translating them in this file would tear them off it,
and the next render would put them back.

**The project's working notes are not here.** Sailor keeps them in its own
store: `sailor search <words>` finds one, `sailor notes list` shows what is
held, and `sailor notes show <slug>` reads it.
