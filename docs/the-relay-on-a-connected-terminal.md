# The relay on a connected terminal

A session that fills up writes a mandate for whoever comes after it. Today the
mandate gets written and nobody takes it. **Measured on 14/09/2026: eight
mandates wait undelivered in the store's mailroom, twenty-nine more are
archived, and exactly one has ever been taken** — the synthetic session of the
trial of 10/09.

This document says where it stops, and the answer is not the one the plan
predicted. Phase 4 is not missing. It was built, it was proved on a real
terminal, and then **it was switched off by hand, for a good reason**.

## What is already there, and was proved

- **The sensor** (`measure_session`) reads how full a session is, from the
  transcript or the other engine's rollout, and answers `unknown` rather than
  `below` when it cannot see.
- **The contract** (`crates/sessions/src/mandate.rs`) refuses a mandate with a
  blank field while its author is still alive to be asked, and refuses prose
  where a mandate belongs.
- **The arc** (`session-event` as a fourth kind of trigger) evaluates every
  session moment and can start a flow detached. It is alive: on 14/09/2026 the
  store holds 1,441 verdicts for `ask-for-a-mandate`, the newest minutes old.
- **The emptying** (`empty_terminal`, `wait_free`, and the keeper road through
  the terminal-opening application) types the line the command line's own
  descriptor declares, and only once the reading says nobody is being waited
  for in there. Proved live on 10/09 on a session Sailor held, and on 11/09
  through the keeper on a session Sailor did **not** hold.

So the mechanism reaches end to end. What stopped is the thing that uses it.

## Where it actually stops

`~/.config/sailor/flows/empty-a-session-that-handed-on.flow.json` — a file in
the reader's own home, written on 11/09/2026 — **overrides the shipped flow of
the same name**. That is not a defect of the override: it is the decision of
29/08/2026 working exactly as written, *whoever wants a different flow writes
one with the same name in their own home, and theirs wins*.

The home copy has one step, `trigger`, with `source: manual`. It watches
nothing and empties nothing. Its own description says why:

> The shipped flow of this name empties a session as soon as a mandate for its
> terminal is on disk and nobody has taken it. That is not consent. On
> 11/09/2026 it typed the emptying line into a session that had written its
> handover seventeen minutes earlier and had gone on working: the person lost
> the thread of an investigation that was in progress.

The store agrees with the file to the minute. `empty-a-session-that-handed-on`
has 137 trigger verdicts, **all of them between 10/09 16:06 and 11/09 10:24**,
and the home file is stamped 11/09 10:24. `ask-for-a-mandate`, which only asks,
has run continuously through today. One half of the relay was turned off and
the other half was left on, which is why mandates pile up: **the asking half
still asks, and the acting half is a stub.**

**So the missing piece is consent, not mechanism.** Depositing a handover is
something a session is asked to do *while it keeps working*; it is not a
statement that it has finished. Nothing in the contract, and nothing in the
reading, tells those two apart. `wait_free` answers a narrower question — is
anybody being waited for in there — and a session at rest between two turns
looks exactly like a session that is done.

## The second defect, and it is independent

`empty_terminal` receives `cli` from the mandate's own `written.engine`, and
resolves it against the loaded descriptors. A name no descriptor declares is
refused loudly — `unknown_command_line`, «no descriptor of that name is
loaded» — which is the right direction and stops nothing silently.

But **that field is free text the session types itself**. `sailor terminal
mandate` fills in what the session must not have to know — which tree, which
terminal, which store — and takes `engine` as given. Measured on 14/09/2026
over the mailroom:

| what the mandate wrote | mandates waiting | archived | is it a descriptor id? |
|---|---|---|---|
| `claude-code` | 4 | 24 | yes |
| a model's name | 3 | 4 | **no** |
| a command line with a model in brackets | 1 | 1 | **no** |

**Four of the eight waiting mandates, and five of the twenty-nine archived
ones, name a model where a command line belongs.** Turn the relay back on
today and half of them break at that step. The cure is the one the contract
already applies to every other field: refuse at the deposit, while the author
is alive to be asked, instead of at the emptying, when they are gone.

## What phase 5 still is

Lifecycle and declared degradation — fault 147 — is untouched, and nothing
here changes that. It is the only part of the plan that was never built.

## What can be done without the harness, and what cannot

**It cannot be done by asking the command line.** Checked against the
documentation on 10/09/2026: there is no hook, no channel and no signal by
which a running session of it is reset from outside; a message sent between
sessions arrives as text, and a line beginning with a slash is not executed.

**The terminal-opening application is the road, and it is wider than the relay
has used.** Its terminal commands are `list`, `show`, `read`, `send`, `wait`,
`stop`, `create`, `switch`, `close`, `rename`, `split`. The relay uses `list`,
`read` and `send` today, through the `keeps_terminals` block of a descriptor.
It has never used `create`, and `create` takes `--command`: it can **open a
terminal and start an engine in it**.

That changes the shape of the answer. The relay was designed around a
destructive gesture — empty the session that is full — and the destructive
gesture is exactly what got it switched off. With `create`, the successor is
born **beside** its predecessor instead of on top of it:

- nothing is thrown away, so consent stops being the blocking question;
- the predecessor is left for the person to read and close, which is a gesture
  a person is good at and a flow is not;
- the successor starts cold with the mandate, which is what the relay was for.

The keeper road already knows the two hard facts about that application,
measured on 11/09/2026: the pane key a session carries in its environment is
**not** the handle those commands want, and the handle **rotates** — it is
looked up in the list on every call and never kept.

## The first step that can be measured

**A home flow that opens a successor beside a terminal that handed on, and
takes nothing away.** Home, not shipped, because it names a product, and the
repository's flows may not.

The graph, with no new Rust in it:

1. `trigger` on a session event, as the shipped flow already does;
2. `mandate_waiting` — is there a mandate for this terminal nobody has taken?
   The step already exists and already answers with the engine;
3. `shell_check` — find this terminal in the keeper's list, by the pair the
   keepers table holds, looked up fresh;
4. `shell_check` — create a terminal in the same tree with the engine as its
   command;
5. `shell_check` — read the mandate back and confirm it is marked taken.

**The observation that decides it**: the mandate's `taken.by` is filled, within
a declared number of seconds, by a session id that is **not** the depositor's,
**and the depositor is still alive and still holds its context.** Both halves
matter. The first alone is satisfied by the flow that was switched off; the
second is the thing that was lost on 11/09.

**The check that makes it honest**: run it against a terminal for which no
mandate is waiting, and against a mandate whose `engine` names a model rather
than a command line. Neither must open anything. A relay that creates a
successor for a session that handed nothing on is the same fault as the one
that emptied a session that was still working, wearing the opposite costume.

**What it does not settle.** Whether a successor started this way reads the
mandate *well* — a delivered mandate is not an understood one. Whether the
predecessor should ever be closed automatically, which stays a person's
gesture until somebody measures a signal that tells "at rest" from "finished".
And it needs that application to be running: a road that exists only while an
application is up is a road, not a foundation.
