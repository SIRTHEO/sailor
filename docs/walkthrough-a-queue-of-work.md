# A queue of work, in five minutes

Follows `take-the-next-work`: a project's queue, drained one task at a time,
with a person as the fallback when the cheap attempt does not hold. Every
command below was run, from a clone of this repository.

## 0. Which binary, and one variable every path is built from

```
SAILOR_REPO=$(git rev-parse --show-toplevel)
```

Every path below is `$SAILOR_REPO/...`. The `required` field this
walk-through's flow file uses landed in this tree today; a `sailor` already
on your PATH from an earlier build or release does not know it and stops
with `unknown field \`required\``. Build one of your own instead — either
form works, but **use one of them throughout**, never mixed:

```
sailor_bin() { cargo run -q -j 1 --manifest-path "$SAILOR_REPO/Cargo.toml" -p sailor -- "$@"; }
```

or, once you have released one (`sailor release sailor`) built at or after
today, put that binary on PATH and call it plain `sailor`. This walk-through
uses `sailor_bin`, the `cargo run` form, throughout.

## 1. Build the fixtures and seed the queue

```
$ sh "$SAILOR_REPO/flows/tests/make-fixtures.sh"
fixtures written under .../target/fixtures
seed their ledger with: cargo run -p sailor --example seed_take_the_next_work -- .../target/fixtures
```

The script prints its own paths; seed against `$SAILOR_REPO` instead:

```
$ cargo run -q -j 1 --manifest-path "$SAILOR_REPO/Cargo.toml" -p sailor \
    --example seed_take_the_next_work -- "$SAILOR_REPO/target/fixtures"
seeded .../target/fixtures/store
```

It writes one `roles/CHEAP_WORKER` row and four `work-queue` records to
`$SAILOR_REPO/target/fixtures/store`; point every later command at that same
store with `SAILOR_LEDGER`.

## 2. The workspace is already declared — for this repository

This repository ships its own `sailor.json`: nothing to add at the top for a
clone. What is not declared yet is each *fixture project* the queue acts on
— the flow's `execute` step opens a git worktree of whatever project
declared itself nearest to where you stand, via `"tree": "own"`. So the
workspace to init is each fixture, run with that fixture as the working
directory:

```
$ (cd "$SAILOR_REPO/target/fixtures/alpha" && sailor_bin workspace init)
wrote .../target/fixtures/alpha/sailor.json
  name: alpha
  rules: none found
$ (cd "$SAILOR_REPO/target/fixtures/beta" && sailor_bin workspace init)
wrote .../target/fixtures/beta/sailor.json
  ...
```

Skipping this is not silent: without a marker, `execute` cuts its worktree of
*this* repository instead of the fixture, and `acceptance` never finds a
matching entry in `open-worktrees`.

## 3. Declare the worker

The seed points `CHEAP_WORKER` at `claude-code`. Tell which engine is usable
on your machine with `sailor_bin profiles list`. However many rows print,
you need one that is both authenticated and able to write files — every row
you see reads as one of three words:

- **authenticated** — logged in under the profile's own name.
- **authenticated, identity unverified** — logged in, but the engine names
  no account to check against the profile.
- **not authenticated**.

Today codex's shipped recipe runs `exec --sandbox read-only`, and
claude-code's default permission mode refuses to write non-interactively —
so even a row reading authenticated stops before `acceptance` gets anything
to check, unless the engine's descriptor recipe declares the flag that
grants the write (`--sandbox workspace-write` / `--permission-mode
acceptEdits`), a decision for whoever owns that descriptor.

If the engine the seed picked is not your authenticated one, `CHEAP_WORKER`
is a plain ledger row, rewritten directly — there is no `sailor store`
command and no other one that writes a bare role, so this is the only path
today:

```
sqlite3 "$SAILOR_REPO/target/fixtures/store/state.db" \
  "update store set value='{\"tools\":[\"codex\"]}' where collection='roles' and key='CHEAP_WORKER';"
```

## 4. Check the flow before spending anything

Run with `target/fixtures/alpha` as the working directory — `SAILOR_FLOWS`
is needed there because the flow is declared by this repository, not by the
fixture the worktree points at:

```
$ (cd "$SAILOR_REPO/target/fixtures/alpha" && \
    SAILOR_FLOWS="$SAILOR_REPO/flows" SAILOR_LEDGER="$SAILOR_REPO/target/fixtures/store" \
    sailor_bin flow check take-the-next-work)
...
tools asked for: codex
sound command lines (assembled and tried without a prompt, without spending): execute → codex
authenticated homes (asked of the engine, without spending): codex (profile «codex/...», )
missing actions: none
```

## 5. Run it for `alpha` and for `beta`

```
$ (cd "$SAILOR_REPO/target/fixtures/alpha" && \
    SAILOR_FLOWS="$SAILOR_REPO/flows" SAILOR_LEDGER="$SAILOR_REPO/target/fixtures/store" \
    sailor_bin flow run take-the-next-work alpha)
...
sailor flow: flow take-the-next-work ended with status failed; run take-the-next-work-<id>

$ (cd "$SAILOR_REPO/target/fixtures/beta" && \
    SAILOR_FLOWS="$SAILOR_REPO/flows" SAILOR_LEDGER="$SAILOR_REPO/target/fixtures/store" \
    sailor_bin flow run take-the-next-work beta)
...
sailor flow: flow take-the-next-work ended with status failed; run take-the-next-work-<id>
```

`select` takes the top queued task, `claim` writes it under a key a second
runner's write would refuse, and `execute` hands the task's title, alone, to
the engine on stdin. Both runs above end `failed` at `execute`, for the
reason step 3 already named; `acceptance`, `verdict_state` and `handoff`
never run. An engine actually allowed to write in its own worktree
(checked once, outside Sailor: `claude -p --permission-mode acceptEdits`
created the file in seconds) is what turns `alpha` into `complete`. Each
run's own last line prints its real run id (`take-the-next-work-<id>`, a
nanosecond timestamp) — that id is what `sailor step approve --run <id>
--step <step> --as <your own name>` takes, once a run actually reaches a
step handed to a person.

The project's own fixture-backed test stands in for the live case above —
`alpha` reaching `complete`, `beta` never able to, because its check names a
file the worker is never told to write:

```
$ cargo test -q -j 1 -p sailor --test take_the_next_work --no-fail-fast
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Green here on an idle machine. Under load — a worktree a previous run left
behind, an acceptance that timed out — it has failed, once with
`two_runners_racing_the_same_store_take_each_task_at_most_once` panicking and
its own cleanup unable to remove two worktrees because they "contain
modified or untracked files". That is a known, registered condition, not a
mistake in how you ran it; rerun with `-j 1` against a clean checkout.

A task reaching `verdict_state` with `next_state: parked` is written back
with its `reason`, and `handoff` hands it to a person — never retried on its
own.

## 6. Open the window and close the loop by hand

The desktop window is a separate npm project; its dev command needs its
dependencies installed first, once:

```
$ (cd "$SAILOR_REPO/desktop" && npm install)
added 536 packages ... found 0 vulnerabilities
```

Skipping this is what produces `sh: vite: command not found`: `npm run
desktop` runs `cargo tauri dev`, whose `beforeDevCommand` is `npm run dev`,
which is `vite`, and with no `node_modules` that binary does not exist yet.
The install above took a few seconds here with npm's cache warm; a cold
cache can take a couple of minutes.

```
$ (cd "$SAILOR_REPO/desktop" && npm run desktop)
```

A parked task shows in the attention queue (`AttentionQueue.tsx`, with the
run's own `reason`) and as a slot in the strip (`Strip.tsx`) waiting on a
person. Once a run is actually parked at `handoff` — which needs the writing
engine from step 3 — act on it with the run id its own `flow run` line
printed: `sailor step approve --run <that id> --step handoff --as <your own
name>` (or `reject --why <text>`) — reword and requeue, or leave parked, the
two options `handoff` itself offered — and the row is gone from both views
on the next beat.
