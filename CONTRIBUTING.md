# Contributing to Sailor

Sailor is under construction and used every day by the people writing it, so
interfaces move. What follows is what this tree actually does, not a wish: the
full rules live in [`AGENTS.md`](AGENTS.md) and in `docs/decisioni.md`.

Everything committed here is in English — identifiers, comments, documents,
commit messages, and every message a user of the tool can read. The Italian
still under `docs/` and in older comments is a measured debt, counted by a test
whose number may only fall.

## Build and test

```sh
cargo build --workspace
cargo test --workspace --no-fail-fast
```

Rust 1.89 or newer — the version `rust-version` declares in `Cargo.toml`, not
the newest one you have.

Three things about that battery, each of them already paid for once:

- **`--no-fail-fast` is not a convenience.** Without it `cargo test` stops at
  the first red binary and everything after it never runs — it does not fail,
  it does not start. A run that stops a third of the way through looks green to
  anyone counting failures instead of binaries.
- **Never pipe `cargo test` into `grep` or `tail`.** The exit code becomes the
  last command's, and a red battery reads as a green one. Write the output to a
  file and read the file.
- **The window's shell declares a `[workspace]` of its own**, so
  `cargo test --workspace` never sees it. It has to be asked by name:

  ```sh
  cargo check --manifest-path desktop/src-tauri/Cargo.toml
  cargo test  --manifest-path desktop/src-tauri/Cargo.toml --no-fail-fast
  ```

  Its tests sat outside every gate once, and one of them was red in silence.

**The terminal tests need a real pty.** `crates/terminal` calls `openpty`, and
inside a sandbox that denies it — or denies creating a scratch directory under
the system temp dir — those tests fail for the sandbox and not for your change.
If that is the only red you have, say so; do not report a green you did not
measure, and do not repair a test that was never broken.

The desktop window itself is `desktop/` (Tauri and React), outside the
workspace, and the README explains how to run it.

## The ratchet, and it is the gate before every commit

```sh
cargo build -p sailor && ./target/debug/sailor ratchet
```

Read the verdict before you commit. This is the single most important thing to
know about working in this tree.

A **judge** is a test that reads the sources instead of exercising the code: how
much of the tree is comment, whether identifiers are English, whether a path
from somebody's machine got written down, whether a product name appeared in
prose. A **seed** is the number a judge carries in a constant — the count
measured the day it was written, which the tree may not exceed. **Seeds only
fall.** You may lower one when your change removes what it counts; you may not
raise one to make your change fit.

Three seeds are different: they must match the tree **exactly**, in either
direction. They live in
`crates/sailor/tests/the_battery_does_not_shrink_in_silence.rs`:

| seed | what it counts |
|---|---|
| `TEST_FUNCTIONS_TODAY` | every `#[test]` in the tree, the window's shell included |
| `TEST_BINARIES_TODAY` | every `.rs` directly under a package's `tests/` |
| `FLOW_FILES_TODAY` | every `.flow.json`, this project's own and the shipped ones |

Add a test and the ratchet is red until you write the new number; delete one and
it is red too. That is the whole point — a deleted test does not fail, it
vanishes, and nothing was looking for it. Write the number the judge states, in
the same commit, and let the commit message say why the count moved.

`sailor ratchet` does not measure your working tree. It rebuilds
`git archive HEAD` into `target/ratchet-tree`, lays your changed and new files
over that clean copy, and runs each judge there — because several sessions can
write in one checkout, and a seed taken over somebody else's uncommitted file
describes a tree nobody has. It prints the files it laid over: read that list
and check they are all yours. `sailor ratchet --only <judge>` runs one judge.

## Commits

Conventional Commits, as `AGENTS.md` states them: `<type>(<scope>): <subject>`,
lowercase, imperative, no trailing period. From the log:

```
fix(live): the port is asked of the port, not only of the ledger
test(ratchets): every failure prints all three numbers, not the one that fell
refactor(profiles): the engines a profile can be made of are a file, not Rust
```

- **One self-standing piece per commit.** A commit is a thing that holds up on
  its own, not a save point: if the subject needs an "and", it is two commits.
- **The body carries the why.** The what is in the diff. The story of how it
  went — the dates, the earlier shape, what you tried first — belongs here,
  where it keeps the real author and the real date, and not in a comment.
- No tooling attribution trailers.

## Comments

- **English**, like the rest of the tree.
- **At most six lines per block.** The measure is
  `cargo test -p sailor --test comments_do_not_crowd_out_the_code`, and its
  numbers can only go down. The cap is not taste: the semantic index embeds
  comment text as written, so a long block becomes the chunk a search compares
  against your question instead of the code under it.
- **A comment says why, not what.** If renaming a variable, extracting a
  function or writing a test would say it, do that instead. A comment earns its
  place when a why is left that the code cannot carry: an external constraint, a
  counter-intuitive choice, a declared limit.
- **No dates and no chronicle.** Not "it used to do X", not "changed on such a
  day". At most a pointer: `// see fault 39`.
- **No product name in prose**, and no framing of this work against somebody
  else's. A product name is a fact inside a descriptor and the vocabulary of the
  ban inside a gate; in prose it is counted by
  `a_product_name_in_prose_only_ever_falls`, and the count only falls.
- **Nothing from your machine.** No absolute path from your home, no client or
  employer name, no internal repository name, no transcript copied out of
  private tooling. `nothing_from_this_machine_is_published` reads your `HOME` at
  run time and goes red if it is written anywhere a reader can open.

## Tests

**A change to behaviour needs a test**, and a test counts only if it could have
come out differently. So the practice has a name and a shape: **put the defect
back.** Write the test, watch it born red, make the repair, then re-insert the
original defect and watch that same test turn red again. If the tree stays green
with the defect back in, the test is not testing anything, whatever its name.

Say in the pull request which line you put back and what went red. A test never
seen to fail is not a test.

## Branches

- **`sorgenti` is the trunk**, and the only branch that releases. `main` exists
  and is not it: it is the history from before the rewrite.
- **Work goes on `work/<short-topic>`**, in English, named for the work and not
  for the day or the gesture — `work/terminal-claims`, not `work/evening-merge`.
- **A branch is born from `sorgenti`, returns to `sorgenti`, and dies.** Do not
  open a branch whose only job is to merge; merge where the work already
  integrates.
- **Whoever opens a worktree closes it.** `git worktree remove <directory>`
  takes the directory away and leaves the branch where it is; deleting the
  branch is a separate decision, and it is proved by content, not by ancestry.

## Pull requests

Keep it short, and keep both halves:

- **What it resolves.** The motivation, in your words: the defect, the missing
  behaviour, or the decision it implements. Link the issue or the fault number
  if there is one.
- **How to test it.** The commands you actually ran, and their verdict — the
  battery, the window's shell if you touched it, and which test you saw go red
  with the defect put back.
- **The ratchet.** Say it is green on your `HEAD`, and if a seed moved, say
  which one and to what number.

A reviewer who did not write the change gives the verdict on it. Whoever makes a
thing does not judge it, so tell us what you wrote, how you proved it, and what
is still uncertain — the uncertainty is useful and hiding it is not.

## Use of AI

This project is largely built by agents, and it is what Sailor is for. So an
AI-assisted contribution is welcome here, plainly and without a disclaimer
ritual.

One condition, and it is the whole condition: **you must be able to explain the
change in your own words** — why it is the right repair, what else it touches,
and how you know it works. If you cannot, the change is not ready, whatever
wrote it. Everything above applies unchanged to what an agent produced: the same
tests, the same ratchet, the same commit style, and no attribution trailer.
