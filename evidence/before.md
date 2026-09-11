# The state of the tree before anybody touched it

Taken on 2026-09-11 in a worktree of its own, with `CARGO_TARGET_DIR` inside that
worktree (`target/before`). No source file and no test was changed.

## Which commit this actually measures

The assignment names `7bde057c`. **The tree is three commits past it.** `main`
and this worktree both stand at `3b58c759`, and `7bde057c` is its ancestor:

```
3b58c759 feat(window): the column names its own parts, and the capture can reach them
329aeabd feat(workspace): the whole left column is one reading, and a command prints it
81da1e4f feat(terminal): the host holds a terminal past its client, and asks who is writing
7bde057c feat(relay): the handle a keeper wants is looked up, never assumed
```

Everything below is `3b58c759`. Two of the four reds are the work of those three
commits, so the difference is not cosmetic: measured at `7bde057c` the tree would
show two reds, not four.

## The one-line answer

| battery | verdict | file |
| --- | --- | --- |
| `cargo test --workspace --no-fail-fast` | 17 targets failed, **4 of them real** | `cargo-test-workspace.txt` |
| `npx tsc --noEmit` | green | `desktop-tsc.txt` |
| `npm test` | green, 604/604 | `desktop-npm-test.txt` |
| `npm run screenshots` | green, 12 captures, none missing | `desktop-screenshots.txt` |
| `npm run check:canvas` | green | `desktop-check-canvas.txt` |
| `npm run check:bundle` | **RED** | `desktop-check-bundle.txt` |

## The Rust battery

197 test-result lines (test binaries plus doc targets). **2025 passed, 55 failed,
2 ignored**, across 17 failing targets.

**51 of the 55 are the perimeter, not the tree.** They are the two families
AGENTS.md already names, and both say so in their own panic text: `openpty`
refused (`Operation not permitted`, 12 tests) and `mkdir /tmp/sr-*` refused
(`PermissionDenied`, 39 tests). Proof rather than assertion: the binary
`a_terminal_is_held_by_the_host`, which contributes 7 of them, was run again
outside the sandbox and came back **7 passed, 0 failed** — `second-run-of-a-sandbox-red.txt`.

### The four real reds, each reproduced twice

Re-run from the already-built binaries, outside the sandbox, single-threaded:
`second-run-of-the-reds.txt`. All three that could be re-run failed **identically**
both times. None of them is load or the machine.

1. **`a_sentence_for_a_person_lives_in_the_catalogue`** — a ratchet that has risen.
   `sentences written into the code: 7 (the declared number is 3)`. The four new
   ones are in `crates/sailor/src/workspace_cmd.rs` (3), `release_cmd.rs`,
   `remaining_cmd.rs`, `session_cmd.rs`, `toml_graft.rs` (1 each). A sentence a
   user reads has to have a `cli.*` or `ui.*` key in `i18n/en.json` and
   `i18n/it.json`. Seed at `crates/sailor/tests/a_sentence_for_a_person_lives_in_the_catalogue.rs:13`.

2. **`every_power_the_window_declares_is_one_it_can_reach`** — `1 of 62 commands
   are declared and never asked for, 1 more than the seed's 0: ["left_column"]`.
   `left_column` arrived with `329aeabd`, one of the three commits past the base.
   The shell declares the command; no page in the window asks for it.

3. **`the_quota_reader_and_the_descriptor_name_the_same_engine`** — «codex»
   declares a `quota` block and does not announce it. A descriptor disagrees with
   the reader; it is data, not code. Arrived with `cef23688`/`fe21a91d`.

4. **`the_shell_of_the_window_still_compiles`** — `the shell does not compile, and
   no workspace test builds it. The compiler's first word: error: could not
   compile 'cfb' (lib)`. **This one I could not run a second time**, and its cause
   is not in what the test prints: the test surfaces only the compiler's first
   line. See the disk note below — it is the one red where a machine cause is
   still on the table and has not been ruled out.

### What a red does NOT mean here

Nothing in this run is intermittent. No test failed once and passed once.

## The desktop battery

- `npx tsc --noEmit` — clean, exit 0.
- `npm test` — **81 files, 604 tests, 604 passed**, 36.7 s.
- `npm run screenshots` — 12 captures (6 scenes × 375 px and 1440 px), and
  `missing.txt` says `no scene missing: all reached and captured.`
- `npm run check:canvas` — green at 375, 760, 1100 and 1440 px. **Read what it
  measured**: every row says `panel null`. The script never selects a step, so
  the inspector is never open, so prohibition 11 is only ever checked in the case
  where nothing is competing for the width. See `the-charter-holds.md`.
- `npm run check:bundle` — **RED, and it is the only red in the window**:

```
index-v-8ZIeb2.js is 775.66 kB against a ceiling of 748 kB.
Move a section behind `React.lazy`, or say in the commit why the ceiling rises.
```

27.66 kB over. Nine chunks are already lazy; the entry chunk is not.

## The counts, as the ratchets measure them today

`the_battery_does_not_shrink_in_silence` allows a seed to sit **zero** away from
the tree. Counted by hand on this worktree, every one matches:

| what | seed | measured |
| --- | --- | --- |
| test binaries (`*.rs` directly under a package's `tests/`) | 153 | 153 |
| `#[test]` functions (`crates/` + `desktop/src-tauri`) | 2144 | 2079 + 65 = 2144 |
| flow files (`flows/` + `crates/flow/system/`) | 19 | 19 |
| window tests (`test(` at line start under `desktop/src`) | 604 | 604 |

Other seeds and where they stand:

| judge | seed | verdict |
| --- | --- | --- |
| `no_crate_warns_more_than_today` (clippy) | per crate, in the test | green, 129.5 s |
| `no_crate_lets_its_comments_outtalk_its_code_more_than_today` | 412 long blocks | green |
| `a_product_name_in_prose_only_ever_falls` | 5 mentions | green |
| `no_engine_is_named_in_the_code` | 5 named, 0 identifiers | green |
| `files_do_not_grow_out_of_scale` | 0 over 2000 lines | green |
| `a_sentence_for_a_person_lives_in_the_catalogue` | 3 | **RED at 7** |
| `every_power_the_window_declares_is_one_it_can_reach` | 0 | **RED at 1** |

**No clippy warning stands above its seed.** The clippy ratchet is green.

## Two things that are not a test result and change how to read one

**The disk has 452 MB free of 460 GB.** Measured while the battery was running;
it was 1.5 GB at the start of it and the drop is partly mine — `target/before` is
4.4 GB. A background write to the task directory failed with `ENOSPC` mid-run. No
`ENOSPC` appears in the battery's own output, so I cannot pin the `cfb` compile
failure on it, but I also cannot rebuild the shell to find out without evicting
somebody else's data, which is not mine to do. **Whoever integrates should free
the machine before believing any compile result from today.**

**The machine is loaded**: load average 12.92 at the start, with two other
terminals alive in this checkout and 57 flow runs stopped mid-step. That did not
produce any of the four reds — each reproduced — but it is why the battery took
what it took.

## The token ceilings are seeded at zero

`token-seeds.json` covers 19 flows. **Seventeen of them have `runs_measured: 0`
and `tokens_a_run: 0`.** Only `dispatch-the-work` (4 runs, 130,894) and
`draft-a-flow` (4 runs, 105,816) have ever been measured. A ceiling of zero from
zero runs is not a ceiling anybody has tested.

## The commands, verbatim

```sh
CARGO_TARGET_DIR="$PWD/target/before" cargo test --workspace --no-fail-fast
# in desktop/
npx tsc --noEmit
npm test
npm run check:bundle
npm run screenshots
npm run check:canvas
```

`desktop/node_modules` was absent in this worktree. It was copied from the main
checkout, whose `package-lock.json` is byte-identical to this one. It is ignored
by git and no tracked file was touched.

The three `tsx` scripts (`check:bundle`, `screenshots`, `check:canvas`) **cannot
run under the agent sandbox**: `tsx` opens a unix socket for its own IPC and the
sandbox refuses `listen` with `EPERM`. They were run outside it. This is a
property of the harness, not of the scripts, and no script was edited.
