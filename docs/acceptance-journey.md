# The acceptance journey

A release is a program a person can use, not a suite that passed. This file is the journey a person walks on the **installed** binary before the release is called done; each walk is recorded below with the version it was walked on and what each step actually showed. A step whose expectation is not met is red, and a red step stops the release, whatever the suite said.

Every path is `$SAILOR_REPO/...` with `SAILOR_REPO=$(git rev-parse --show-toplevel)`. Every command of the walk uses the installed `sailor` on PATH, never `cargo run`; step 0 alone, which comes before the release, runs a binary it builds from the trunk being released. The store used is a scratch one under `target/`, never this machine's own, save in steps 0 and 10, which hold the public page to the store it was counted from.

## Before the release is cut

Step 10 checks the public page as it is committed, so the page is brought up to date before the release is cut, never during the walk. Step 0 is done by hand, by whoever cuts the release; no flow runs it.

0. Cut a tree for a branch that starts at the trunk being released, named for the version and the day so it meets no branch an earlier release left standing, and cut it through `sailor worktree create`, which writes the tree down so it can be closed. Build that trunk into a target of its own, taking the machine's turn as every build does. Render the page from this machine's own store, with `SAILOR_LEDGER` unset so no scratch store stands in for it, and check it:

   ```sh
   branch=work/the-page-counted-for-<version, dots as hyphens>-on-$(date +%Y-%m-%d)
   git -C $SAILOR_REPO fetch
   git -C $SAILOR_REPO branch $branch origin/$(git -C $SAILOR_REPO config --get sailor.trunk)
   tree=$(cd $SAILOR_REPO && sailor worktree create $branch) && cd $tree
   CARGO_TARGET_DIR=$tree/target/page sailor machine turn -- cargo build -j 1 -p sailor
   env -u SAILOR_LEDGER target/page/debug/sailor faults render --open --file docs/faults-encountered.md
   env -u SAILOR_LEDGER target/page/debug/sailor faults check docs/faults-encountered.md
   git commit -m "docs(faults): the public page counted for <version>" -- docs/faults-encountered.md
   ```

   The render writes the whole file from the page the fault store's crate holds, its words included, and keeps nothing the file held. The check must say «agrees». The page reaches the trunk through an ordinary pull request, the release is cut from the trunk that holds it, and the tree is closed with `sailor worktree close ${branch#work/}`.

   The first time this runs, it is the first thing that writes the history's triggers into the machine's store. The binary in service, older than them, keeps working against them: add, close, reopen, summary, reword, link, unlink and list each ran against a store that held them, in about 15 ms a command, and every change they made to a fault or to its summary was written down. The check reads the history of the store it is given. A page rendered from a store whose history does not reach the page's stamp answers «cannot tell», and so does a page rendered from a copy taken before this store's history began. A copy taken later holds the start of the store's history: a page rendered from it agrees while the copy and the store have not parted, and once they have, the check names the first line that differs or cannot tell. Only a page rendered from the store itself is sure to agree.

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
| 9 | `sailor step approve --run <beta run id> --step handoff --as <name>` | «step handoff approved»; the run ends `failed` with «the required step acceptance did not pass», because an approval does not make the acceptance pass; `beta`'s queue row stays `parked`, the option the approval leaves standing; the run leaves the attention list: it names a waiting run's handed steps, runs stopped at their cap, dead terminals, unreachable engines and an unreadable store, and a run that ended `failed` is none of these |
| 10 | `env -u SAILOR_LEDGER sailor faults check docs/faults-encountered.md` on the page as committed at the candidate, with no render first; the one step read against this machine's own store | «agrees with the store as it stood through fault N»: the check reads only the stamp from the page and compares the whole page, byte for byte, with a render of the store as it stood at its stamp; the render writes every line itself, the opening prose included, and takes nothing from the page. Faults opened or closed after the page was counted, by any binary, do not move it. The first line that differs is named and is red; «cannot tell what stood then» is red too: the page was not counted from this store's history, which step 0 is for. The walk leaves the page as committed |

## Walks

| date | version (commit) | steps green | red steps, and what was seen | walked by |
|---|---|---|---|---|
| 2026-09-14 | 0.1.0 (b3d8efdf) | 2, 3, 4, 5, 6, 7, 8, 9 | 1: `sailor version` prints «sailor 0.1.0» and no commit, so a person cannot tell which build is in service. 10: the open count matches (54) but the store says 177 faults where the register says 186 — rows added to the document were never added to the store. And the walk itself broke something: the window opened on the scratch store ran the machine's scheduled flows against it and took down a worktree a live terminal was working in (fault 187). | the coordinator |
| 2026-09-22 | 0.1.0 (82859477) | 1, 2, 3, 4, 5, 6, 7 | 10: the store holds 88 open faults and the page says 87, because fault 274 was opened after the page was rendered; the page is rendered again from the store with this change. 2 as written seeded a real engine as the cheap worker, so 6 and 7 first called it and spent about a dollar (fault 275); both passed once the fake worker was named. 8 and 9 not walked, the release having stopped at 10. | the coordinator |
| 2026-09-23 | 0.1.0 (3a03bdae) | 1, 2, 3, 6, 7, 9 | 10: the store holds 126 open faults and the page counts 88, because faults are opened all day and the page is frozen at the commit that rendered it: the step compared a moving store with a fixed page and was red by construction. It now compares the page with the store as it stood when the page was counted. 4 as written: one engine reads not known, since its descriptor declares no way to ask it; the expectation now names that answer. 5 as written: the fixture worker's descriptor contradicted itself and declared no refusal, so its line was left untried; the fixture now declares both. 9: the run ends `failed` and the row stays `parked`, which is right and is now what the step says. 8 not walked. | the coordinator; the window was not opened |
