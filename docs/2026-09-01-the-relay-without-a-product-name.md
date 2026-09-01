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
writes output, not input. So `sailor accompany run -- <cli>` opens a
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
that was missing in 262 sessions out of 348. `sailor accompany mandate` reads it
from standard input instead.

## What exists, with the command that shows it

| piece | how to see it |
|---|---|
| the conduit | `sailor accompany run -- <cli>`, then `sailor accompany press --tty <name> --text "…"` from anywhere else |
| how full | `sailor accompany list --ceiling 500000` |
| emptying | `sailor accompany reset --tty <name> --cli <id>` |
| the mandate | `sailor accompany mandate` from inside the session |
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
from transcripts, and the first honest experiment is to accompany a command
line that reports its own usage and compare. `Model::between` exists for
exactly that and nothing calls it yet.

**A terminal does not survive the window.** The pipe is held by the accompanying
process. Closing the window ends it, and `docs/2026-09-01-il-contratto-del-terminale.md`
already names the separate job that would fix it.

## The one thing to check first

The conduit. Everything else is composed from it, and if typing into a live
session turns out to be unreliable in daily use, the sequence above is a
sequence over something that does not work. The tests type into a real
pseudo-terminal and into a real `sailor accompany run`, but nobody has yet run
a working day inside one.
