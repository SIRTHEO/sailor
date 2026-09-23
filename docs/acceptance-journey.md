# The acceptance journey

A release is a program a person can use, not a suite that passed. This file is the journey a person walks on the **installed** binary before the release is called done; each walk is recorded below with the version it was walked on and what each step actually showed. A step whose expectation is not met is red, and a red step stops the release, whatever the suite said.

Every path is `$SAILOR_REPO/...` with `SAILOR_REPO=$(git rev-parse --show-toplevel)`; every command uses the installed `sailor` on PATH, never `cargo run`. The store used is a scratch one under `target/`, never this machine's own.

## The steps

| # | what the person does | what they must see |
|---|---|---|
| 1 | `sailor version` | the version and the commit the binary was built from; they match the release note |
| 2 | `sh flows/tests/make-fixtures.sh` then `cargo run -q -j 1 -p sailor --example seed_take_the_next_work -- target/fixtures fake-cheap-worker`, then the `export` line the script prints | fixtures written, with the fake worker and its descriptor; «seeded .../target/fixtures/store»; the `CHEAP_WORKER` role names the fake worker, so no step of the journey calls a paid engine |
| 3 | `(cd target/fixtures/alpha && sailor workspace init)` and the same for `beta` | «wrote .../sailor.json» twice |
| 4 | `sailor profiles list` | one row per profile, each reading authenticated, authenticated with identity unverified, NOT AUTHENTICATED, or not known followed by what is missing; no row is silent. An engine whose descriptor declares no `login_status` reads not known, because nothing can ask it and a guessed answer could call an empty home signed in; a MISMATCHED row is red |
| 5 | `(cd target/fixtures/alpha && sailor flow check take-the-next-work)`, in the environment step 2 exported | «missing actions: none»; «tools asked for: fake-cheap-worker»; «sound command lines» lists `execute → fake-cheap-worker`; no descriptor is said to contradict itself |
| 6 | `(cd target/fixtures/alpha && sailor flow run take-the-next-work alpha)`, in the same environment: the fake worker writes the file the check names | the run ends «complete»; `alpha`'s queue row is closed |
| 7 | the same for `beta` (its check names a file the worker is never told to write) | the run ends «waiting» at `handoff`; the row carries the reason the acceptance did not pass |
| 8 | the window (`cd desktop && npm run desktop`, with the same `SAILOR_LEDGER`) | the parked `beta` row is visible with its reason and a way to open the run; nothing else claims attention |
| 9 | `sailor step approve --run <beta run id> --step handoff --as <name>` | «step handoff approved»; the run ends `failed` with «the required step acceptance did not pass», because an approval does not make the acceptance pass; `beta`'s queue row stays `parked`, the option the approval leaves standing; the run leaves the attention list, which holds waiting runs only |
| 10 | `sailor faults check docs/faults-encountered.md` (the machine's own store) | «agrees with the store as it stood through fault N»: every row on the page is a fault the store held open when the page was counted, and the count sentence equals the open count at that moment. Faults opened or closed after the render do not move it; a row the store never held is red |

## Walks

| date | version (commit) | steps green | red steps, and what was seen | walked by |
|---|---|---|---|---|
| 2026-09-14 | 0.1.0 (b3d8efdf) | 2, 3, 4, 5, 6, 7, 8, 9 | 1: `sailor version` prints «sailor 0.1.0» and no commit, so a person cannot tell which build is in service. 10: the open count matches (54) but the store says 177 faults where the register says 186 — rows added to the document were never added to the store. And the walk itself broke something: the window opened on the scratch store ran the machine's scheduled flows against it and took down a worktree a live terminal was working in (fault 187). | the coordinator |
| 2026-09-22 | 0.1.0 (82859477) | 1, 2, 3, 4, 5, 6, 7 | 10: the store holds 88 open faults and the page says 87, because fault 274 was opened after the page was rendered; the page is rendered again from the store with this change. 2 as written seeded a real engine as the cheap worker, so 6 and 7 first called it and spent about a dollar (fault 275); both passed once the fake worker was named. 8 and 9 not walked, the release having stopped at 10. | the coordinator |
| 2026-09-23 | 0.1.0 (3a03bdae) | 1, 2, 3, 6, 7, 9 | 10: the store holds 126 open faults and the page counts 88, because faults are opened all day and the page is frozen at the commit that rendered it: the step compared a moving store with a fixed page and was red by construction. It now compares the page with the store as it stood when the page was counted. 4 as written: one engine reads not known, since its descriptor declares no way to ask it; the expectation now names that answer. 5 as written: the fixture worker's descriptor contradicted itself and declared no refusal, so its line was left untried; the fixture now declares both. 9: the run ends `failed` and the row stays `parked`, which is right and is now what the step says. 8 not walked. | an agent; the window was not opened |
