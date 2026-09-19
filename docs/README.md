# What is here

Six pages, and only because something in this tree opens them by name: a judge,
a shipped flow, a compiled constant or a page the code is written against. A
document is named for what it says; when it was written is what `git log` says,
with the real author and the real date.

## Understand

- [`decisions.md`](decisions.md) — the architecture decisions, as numbered
  records. The six permanent constraints come first and are the yardstick every
  other decision is judged by. Read it before repairing anything, or a road
  already discarded gets taken again.

## Operate

- [`the-delivery-loop.md`](the-delivery-loop.md) — the ten steps from taking
  work to closing it, the same from a terminal or as a recorded flow. Four
  shipped flows name their step number in it.
- [`the-terminal-contract.md`](the-terminal-contract.md) — the seven commands
  the bridge exposes, the two events, and who owns which files. The Rust half
  and the React half are both written against this page; whoever finds it
  differing from the code opens a fault.
- [`acceptance-journey.md`](acceptance-journey.md) — the steps a person walks
  on the installed binary before a release is called done, and the record of
  every walk. `cut-a-release` hands this page to whoever walks it.

## Contribute

- [`gates.md`](gates.md) — the lettered gates, what each one measures and when
  it applies. `scripts/run-gates.sh` runs them and two shipped flows digest
  this page to prove a review was made against the version it names.
- [`faults-encountered.md`](faults-encountered.md) — the open faults that
  carry a summary written for users, one line each. The page is generated from
  Sailor's own fault store by `sailor faults render --open`; the full register,
  with how each fault came to light and **what would have stopped it**, stays
  in that store.

## What is not here

**Essays, walkthroughs, studies and the window's mock-ups are not here.** A
public repository carries the product, not the working material that led to it.
Sailor keeps that material in its own store: `sailor notes list` shows what is
held, `sailor notes show <slug>` reads it, and `sailor search <words>` finds
one. The renders and prototypes of the window live beside the notes, outside
the tree.

**The full fault register is not here.** Sailor keeps it in its own store,
with how each fault showed and what would have prevented it: `sailor faults
list` reads it. Only an open fault given a summary for users, with `sailor
faults summary`, is rendered into `faults-encountered.md`.
