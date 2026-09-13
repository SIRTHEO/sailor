# A queue of work, in five minutes

Follows `take-the-next-work`: a project's queue, drained one task at a time,
with a person as the fallback when the cheap attempt does not hold. Every
command below was run, from a clone of this repository, against a binary
built locally with `cargo build -p sailor`.

## 1. The workspace is already declared

This repository ships its own `sailor.json`: nothing to add at the top for a
clone. What is not declared yet is each *project* the queue acts on — and the
flow's `execute` step opens a git worktree of whatever project declared itself
nearest to where you stand, via `"tree": "own"`. So the workspace to init is
each fixture project:

```
$ cd target/fixtures/alpha && sailor workspace init   # and again for beta
wrote .../target/fixtures/alpha/sailor.json
  name: alpha
  rules: none found
  checks: none (they are written by hand: guessing them would be deciding them for you)
```

Skipping this is not silent: without a marker, `execute` cuts its worktree of
*this* repository instead of the fixture, and `acceptance` never finds a
matching entry in `open-worktrees`.

## 2. Build the fixtures and seed the queue

```
$ sh flows/tests/make-fixtures.sh
fixtures written under .../target/fixtures
seed their ledger with: cargo run -p sailor --example seed_take_the_next_work -- .../target/fixtures
check the flow against it with: SAILOR_LEDGER=.../target/fixtures/store cargo run -p sailor -- flow check take-the-next-work
```

Run the two lines it prints. The seed writes one `roles/CHEAP_WORKER` row and
four `work-queue` records to `target/fixtures/store`; point every later
command at that same store with `SAILOR_LEDGER`.

## 3. Declare the worker

The seed points `CHEAP_WORKER` at `claude-code`. There is no `sailor tools`
command; the tool ids Sailor knows are the ones in
`crates/toolbox/descriptors/default.json`. Tell which is usable on your
machine with `sailor profiles list` (AUTHENTICATED / NOT AUTHENTICATED per
engine) or read the same fact off `sailor flow check`'s own
`HOMES WITHOUT CREDENTIALS` line, below. If the seeded engine is not the
authenticated one, the role is a plain ledger row, rewritten directly since
there is no CLI for it yet:

```
sqlite3 target/fixtures/store/state.db \
  "update store set value='{\"tools\":[\"codex\"]}' where collection='roles' and key='CHEAP_WORKER';"
```

## 4. Check the flow before spending anything

Run from inside `target/fixtures/alpha` — `SAILOR_FLOWS` is needed there
because the flow is declared by this repository, not by the fixture the
worktree points at:
```
$ SAILOR_FLOWS=$PWD/flows SAILOR_LEDGER=$PWD/target/fixtures/store sailor flow check take-the-next-work
tools asked for: codex
sound command lines (assembled and tried without a prompt, without spending): execute → codex
authenticated homes (asked of the engine, without spending): codex (profile «codex/...», )
```
No missing actions, no cycles: the graph and the credentials are sound before
a single call is placed.

## 5. Run it for `alpha` and for `beta`

```
cd target/fixtures/alpha && SAILOR_FLOWS=.../flows SAILOR_LEDGER=.../store \
  sailor flow run take-the-next-work alpha
```

`select` takes the top queued task, `claim` writes it under a key a second
runner's write would refuse, and `execute` hands the task's title, alone, to
the engine on stdin. Here is the one honest caveat: a real coding engine is
not the deterministic stand-in the project's own test uses. Measured:
`codex`'s shipped recipe runs `exec --sandbox read-only`, so it cannot create
the file `acceptance` looks for — one run said so in its own words before
breaking on its account's quota, another broke on the thirty-second timeout
while still reading this repository's own `AGENTS.md`. Either way `execute`
never reached `Went`, so `acceptance`, `verdict_state` and `handoff` never
ran: the graph refuses to call a task closed before its acceptance command
has passed. An engine actually allowed to write in its own worktree (checked
once, outside Sailor: `claude -p --permission-mode acceptEdits` created the
file in seconds) is what turns `alpha` into `complete`.

The deterministic proof that the graph itself does the right thing — `alpha`
reaching `complete`, `beta` never able to, because its check names a file the
worker is never told to write — is the project's own fixture-backed test:

```
$ cargo test -p sailor --test take_the_next_work
test two_runners_racing_the_same_store_take_each_task_at_most_once ... ok
test without_a_claim_two_runners_can_both_declare_the_same_task_done ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

A task reaching `verdict_state` with `next_state: parked` is written back
with its `reason`, and `handoff` hands it to a person — never retried on its
own.

## 6. Open the window and close the loop by hand

```
cd desktop && npm run desktop   # the script runs `cargo tauri dev`
```
A parked task shows in the attention queue (`AttentionQueue.tsx`, with the
run's own `reason`) and as a slot in the strip (`Strip.tsx`) waiting on a
person. Act on it with `sailor step approve --run <run> --step handoff --as
<you>` (or `reject` with `--why`) — reword and requeue, or leave parked, the
two options `handoff` itself offered — and the row is gone from both views on
the next beat.
