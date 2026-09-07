# Time is the last choice: the four levels, applied to the life cycle of a branch

**01/09/2026.** Born from a request of Theo's — *«if we need to, let us do the
time nodes, timers, cron that fire on the dot every so often»* — and from the
answer research gave instead of the one asked for.

The **measured facts** are kept apart from the **decisions**, which are Theo's.
Where a number appears without the word «measured», it is a reading of the code,
not a test that was run.

## The rule already exists, and we did not write it

The rule was not invented here, and it starts from this: a cleanup cron is
almost always the symptom of a badly modelled data life cycle.

It is the same thing Theo said on 31/08 with an image: *if I fall ill because I
stand at −10 in a t-shirt, I do not buy the medicine — I buy the clothes*. The
rule turns it into an **order of preference**, and cron is the fourth:

1. **The orphan must not be able to exist** — a constraint, or a coordinated
   deletion in the very gesture that closes the thing.
2. **An event says when to act** — whoever does the thing emits the signal.
3. **It expires on read** — nobody sweeps: whoever looks finds the old thing
   already declared old.
4. **Cron** — only if intrinsically periodic and with no alternative, or as a
   **safety net** for an event-driven path that can lose the signal.

With two lines that count for more than the rest: *«a cron that gets added must
have a comment explaining why steps 1-3 do not apply»*, and **«cron is never the
engine — whoever enqueues has to wake»**. A queue drained only by the tick has
the tick's interval for latency, always.

## The cost of not having the first three levels

The case that produced this section was measured elsewhere and is not published
here, so take the ranking above on its reasoning and not on numbers this
document does not show. What carries over is the mechanism, and it is general.

Once a branch is orphaned, the question «has this work already arrived somewhere
else?» has no cheap answer. Where requests are merged by squashing, the branch
is **never** an ancestor of the trunk, so `git cherry` and `--contains` answer
«not merged» even for work that has been in for weeks. What is left is comparing
the content file by file — one reading per branch, and a second opinion on every
verdict, because a wrong «already in» deletes work.

That is the whole argument for the first three levels. The expensive part is not
the sweeping, which a cron does for nothing; it is that by the time anything
sweeps, **the knowledge of what the thing was for is gone**, and it has to be
reconstructed from the content by whoever did not write it. Levels 1 to 3 all
act while that knowledge is still in the room.

## The four levels applied to a branch

### Level 1 — the branch cannot be left an orphan

Whoever merges the request closes the branch in the same gesture. There is
nothing to watch because nothing to watch is ever born. On GitHub this is a
checkbox in the repository settings, not a flow: **the cheapest place where the
problem gets solved is not inside Sailor.** It is worth writing here, because a
system that means to govern a life cycle has to be able to say «this piece is
not mine» too.

### Level 2 — the merge is an event

Whoever merges says so, and the cycle moves on at once. A flow that reacts to an
external fact demands a power Sailor **does not have** today: a trigger that
lights up on something happening elsewhere.

### Level 3 — it expires on read

When somebody opens the list of branches, the ones past their termination
condition declare themselves expired right there, without anybody having swept.
It is the level with the best ratio of value to cost, and it **demands one thing
only**: the life-cycle declaration deposited at the branch's birth — what closes
it, what is lost if it disappears. The flow that deposits it is already written:
`~/.config/sailor/flows/<progetto>/un-ramo-dichiara-come-finisce.flow.json`.

### Level 4 — the timer, as a net

It comes round every so often and picks up what the first three lost. **If the
first three exist, the precision of the tick matters little**: in a production
bot the net runs every five minutes and is the engine of nothing.

## What Sailor has, and what it lacks — measured on 01/09/2026

| needed for | Sailor today |
|---|---|
| depositing the declaration | **there**: `store_write` / `store_read` / `store_list` |
| asking an engine for a judgement | **there**: `external_engine` |
| running a command | **half there**: `shell_check` (see below) |
| lighting up on an external fact | **missing** |
| lighting up on time | **missing** |

**`shell_check` can say *whether*, not *what*.** The output of the step is
`{"status": ...}` and no more (`crates/actions/src/lib.rs`, branch `Ok(ActionOutcome::Went(json!({ "status": status })))`).
The command's output does not reach the flow. So «does the branch still exist?»
can be asked; «which request concerns it, and is it merged?» cannot. It is the
fifth of the powers that survey had already isolated — *returning a value
instead of an outcome*.

**The shapes of trigger are two, and they are code.** `trigger::Kind` has two
variants only, `manual` and `terminal`; the list of descriptors is data — a JSON
goes into `~/.config/sailor/triggers.d/` — but a new **shape** is one more
variant in the enum. And `terminal` today is declared and does not listen, with
a sentence this document makes its own:

> «A simulated listening would be worse than an absent one, because a green flow
> would say somebody had spoken.»

## If and when the time node gets written, five things have to be decided first

They are not implementation details: they change what the node *is*.

1. **Who keeps the time.** Sailor running and counting (it does not fire with
   the window closed); the operating system waking it (precise, but Sailor
   installs something outside itself); or Sailor looking at start-up at what was
   missed («every so often» becomes «when I reopen»).
2. **The laptop that sleeps.** «Every 30 minutes» on a machine switched off for
   twelve hours: on waking does it fire twenty-four times or once? Both are
   right, for different jobs — so **the trigger declares it**, the engine does
   not decide it.
3. **An interval and an appointment are two things.** «Every so often» counts
   from the last run and drifts; «at 9» does not drift and demands a time zone.
4. **Who says it did not fire.** It is fault 12 in other clothes: a trigger that
   does not start has to **say so**. If it keeps quiet, «no runs» reads as
   «everything is fine», which is the worst lie of all. In the language of the
   four surfaces: a trigger is a `sense`, and a `sense` has to tell «zero» apart
   from «I could not see».
5. **Two ticks that overlap.** In the bot it was paid for: the event engine does
   not serialise the ticks by itself, and every periodic net carries a
   concurrency limit of one, written by hand. A time node that does not provide
   for it manufactures double runs.

## What is left to Theo

- **Whether level 1 gets switched on in GitHub** — it is a checkbox, and on its
  own it takes away most of the problem. No flow does it better.
- **Which power to build first**: reading a value (a `shell_check` that
  returns), or lighting up by ourselves (the trigger). The first unblocks level
  3, the second levels 2 and 4. **Level 3 costs less and pays off sooner.**
- **The five questions above**, if and when the time node gets written.

## The check that makes this page red

Like every rule written here: *whoever writes a rule also writes what makes it
red*. For this one the check is a test that walks the trigger descriptors and
**fails if a trigger of periodic shape does not declare** what it does when a
run has been missed and what its concurrency limit is. It is born green today,
because no periodic trigger exists — and that is the way this page stays alive
instead of describing what we ought to have done.
