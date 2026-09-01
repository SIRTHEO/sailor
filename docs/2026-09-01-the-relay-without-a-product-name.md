# The relay, built without naming a product

Written on the branch `work/accompagnatore`, six commits, 1,001 tests green
across 70 binaries with `--no-fail-fast`.

The design this follows is `docs/2026-08-28-il-flusso-che-accompagna.md`. Read
that first: it decides the shape, and this only reports what was built and what
was left uncovered.

## What was decided during the work, and why

**The conduit owns the pseudo-terminal; it does not talk to the emulator.**
Theo's constraint: we cannot know which terminals a user has, and adaptations
per emulator are a list forever behind whatever ships next. There is no other
road on this machine — `TIOCSTI` is disabled, and writing to `/dev/ttysNNN`
writes output, not input. So `sailor terminal run -- <cli>` opens a
pseudo-terminal, runs the command line inside it, and bridges it to whatever
window the person is using. The price, and it is one price rather than one per
emulator: the command line has to be started through Sailor.

**The measure counts bytes, not one engine's transcript.** One command line
writes a token count to a file; others write nothing. The affine model over
bytes — 0.68 tokens per byte plus 60,129 already there — has a median error of
6.0% and works for anything that prints.

**The line that empties a context is a descriptor field.** `/clear` belongs to
one command line. It lives in `reset_context`, next to how that command line is
asked a question. Absent means nobody measured it, never that it cannot be
done: the relay refuses by name and says which descriptor to write it in.

**The mandate is written by the session, not scraped from the scrollback.**
Family F of the old relay's faults: the resumption point looked for a phrase
that was missing in 262 sessions out of 348. `sailor terminal mandate` reads it
from standard input instead.

## What exists, with the command that shows it

| piece | how to see it |
|---|---|
| the conduit | `sailor terminal run -- <cli>`, then `sailor terminal press --tty <name> --text "…"` from anywhere else |
| how full | `sailor terminal list --ceiling 500000` |
| emptying | `sailor terminal reset --tty <name> --cli <id>` |
| the mandate | `sailor terminal mandate` from inside the session |
| the sequence | `sailor flow check passa-il-testimone` — six steps, no cycles, no missing actions |
| the beat | `sailor flow tick` — runs what is due, names what it held and why |

## What is not covered, and should not be mistaken for covered

**Nothing invokes the beat.** `sailor flow tick` exists and runs by hand. No
launchd job was installed: this machine has a measured history of services that
do not inherit a path and of `launchctl` answering empty inside a perimeter,
and installing one silently is how a job that never ran gets believed in.

**The relay flow watches one terminal, the one named in its inputs.** It has no
schedule on purpose. A schedule now would buy an automatic no-op that looks
like it works. Watching every accompanied terminal needs a fan-out the engine
does not have.

**Only one shipped descriptor declares how it is emptied.** The others are
silent because nobody has run them and watched a context empty. Filling them
from memory would put a guess where the code cannot tell a guess from a
measurement.

**The estimate has never been checked against a real pipe.** The model comes
from transcripts, and the first honest experiment is to hold the terminal of a
command line that reports its own usage and compare. `Model::between` exists for
exactly that and nothing calls it yet.

**A terminal does not survive the window.** The pipe is held by the process
that opened it. Closing the window ends it, and `docs/2026-09-01-il-contratto-del-terminale.md`
already names the separate job that would fix it.

**A denied scratch directory now accuses the perimeter; a denied syscall still
does not.** Sixteen tests that open a pseudo-terminal or bind a socket fail
inside a sandbox with the operating system's four words, because there the call
is refused rather than the path. Their message is worth the same treatment and
does not have it.

The commit that fixed the first half says that treatment costs one line in
`PtyError`. That is wrong, and the correction is worth more than the claim: the
message a failing test prints comes from the **derived `Debug`**, not from the
hand-written `Display`, because `Result::expect` formats with `{:?}`. Ten types
across eight crates have a written `Display` and a derived `Debug`, and the
crates hold 1,239 `.expect()` calls — so the careful prose of all ten is
invisible in every red test in this repository, while the product, which prints
with `{e}`, shows it correctly. The two surfaces want different things: a person
reading a failure wants the sentence *and* the structure. A `Debug` that
delegates to `Display` would throw away the half that `Debug` is for.

## It was run, and the handover cannot complete

The section below said the conduit was the one thing to check first, because
nobody had run a working day inside one. It has now been run end to end against
a live pseudo-terminal, and that found two things no test had.

**A step that returns `Waiting` never becomes ready again.** `decision_from`
sends `Waiting` to its own bucket and only `Broke` can go back to `ready`, so no
resume and no beat ever re-attempts it. `take_mandate` is built on `Waiting` —
the pause is the whole point of it — so the relay reaches `raccogli-il-mandato`,
parks, and stays parked. Measured: the request reaches the terminal, the step
closes `Waiting` with «the only mandate here is older than this handover», a
fresh mandate is written, and a resume records **no second attempt**.

This was already known in one place and not in the other. The executor carries a
comment saying that for an interrupted step «the one safe choice was waiting —
and a waiting step never becomes ready again, so a resume saw the interrupted
step and never relaunched it». Crash recovery was moved off `Waiting` for that
reason. The relay was then built on it.

**The engine has no outcome that means «not yet, try on the next beat».**
`Waiting` parks forever; `Broke` returns to `ready` in the same loop with no
backoff anywhere, so attempts burn instantly. A patrol flow needs the third
thing, and choosing its shape is a decision about the engine that every action
would inherit — it is not the relay's to make alone.

**And a `with` field must not reference a pointer of its own name.** Every step
here carried `"tty": {"$from": "/tty"}`. References resolve against the composed
input, and `with` is merged over the dependency's output first, so the field
overwrote the value it was about to read and then read itself: `invalid type:
map, expected a string`. Every node already returns `tty`, so a single-dependency
step inherits it and the reference was never needed. Removed. Nothing caught
this because the flow's test checks that its actions exist, not that it runs.

## What did work, measured on a live terminal

A terminal held by `sailor terminal run` with a real program inside; a letterbox
at `ttys021.sock`; a line typed from a process sharing nothing with it but that
socket, and echoed back by the program. 755,222 bytes counted through the pipe,
read as 573,679 tokens, correctly reported as past a 500,000 ceiling. `/clear`
typed from the `claude-code` descriptor rather than from any line in the code. A
mandate written, and a stale one refused by name. A command line that declares no
reset refused by name, and so did typing into a terminal nobody holds.

Under the ceiling the whole chain is skipped and the run stays green with
`counted`, `why` and the measurement recorded — the decline leaves the trace the
old relay never left.

## The one thing to check first

The conduit. Everything else is composed from it, and if typing into a live
session turns out to be unreliable in daily use, the sequence above is a
sequence over something that does not work. The tests type into a real
pseudo-terminal and into a real `sailor terminal run`, but nobody has yet run
a working day inside one.
