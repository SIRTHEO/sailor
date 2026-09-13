# The acceptance journey

A release is a program a person can use, not a suite that passed. This file is the journey a person walks on the **installed** binary before the release is called done; each walk is recorded below with the version it was walked on and what each step actually showed. A step whose expectation is not met is red, and a red step stops the release, whatever the suite said.

Every path is `$SAILOR_REPO/...` with `SAILOR_REPO=$(git rev-parse --show-toplevel)`; every command uses the installed `sailor` on PATH, never `cargo run`. The store used is a scratch one under `target/`, never this machine's own.

## The steps

| # | what the person does | what they must see |
|---|---|---|
| 1 | `sailor version` | the version and the commit the binary was built from; they match the release note |
| 2 | `sh flows/tests/make-fixtures.sh` then `cargo run -q -j 1 -p sailor --example seed_take_the_next_work -- target/fixtures` | fixtures written; «seeded .../target/fixtures/store» |
| 3 | `(cd target/fixtures/alpha && sailor workspace init)` and the same for `beta` | «wrote .../sailor.json» twice |
| 4 | `sailor profiles list` | one row per engine, each reading authenticated, authenticated with identity unverified, or not authenticated; no row is silent |
| 5 | `(cd target/fixtures/alpha && SAILOR_LEDGER=$SAILOR_REPO/target/fixtures/store sailor flow check take-the-next-work)` | «missing actions: none»; the tools asked for and the sound command lines are listed |
| 6 | `(cd target/fixtures/alpha && SAILOR_LEDGER=... sailor flow run take-the-next-work alpha)` with a fake worker descriptor under `SAILOR_TOOL_DESCRIPTORS` that writes the file the check names | the run ends «complete»; `alpha`'s queue row is closed |
| 7 | the same for `beta` (its check names a file the worker is never told to write) | the run ends «waiting» at `handoff`; the row carries the reason the acceptance did not pass |
| 8 | the window (`cd desktop && npm run desktop`, with the same `SAILOR_LEDGER`) | the parked `beta` row is visible with its reason and a way to open the run; nothing else claims attention |
| 9 | `sailor step approve --run <beta run id> --step handoff --as <name>` | the row leaves the attention list on the next beat; the run ends |
| 10 | `sailor faults list --open` (the machine's own store) | the open count and the total match the sentence in `docs/faults-encountered.md` |

## Walks

| date | version (commit) | steps green | red steps, and what was seen | walked by |
|---|---|---|---|---|
| 2026-09-14 | 0.1.0 (b3d8efdf) | 2, 3, 4, 5, 6, 7, 8, 9 | 1: `sailor version` prints «sailor 0.1.0» and no commit, so a person cannot tell which build is in service. 10: the open count matches (54) but the store says 177 faults where the register says 186 — rows added to the document were never added to the store. And the walk itself broke something: the window opened on the scratch store ran the machine's scheduled flows against it and took down a worktree a live terminal was working in (fault 187). | the coordinator |
