# Sailor — instructions for whoever works in this tree

Sailor launches the command lines an engine offers, applies **one single body
of rules to all of them**, and runs every piece of work as a **recorded flow**
instead of a script, a hook or a binary of its own.

You are reading this because you are about to change something here. This tree
refuses a great deal, mechanically, and every refusal below has a name you can
run. Nothing here asks you to be careful: it asks you to measure.

## The route

Five steps, in this order, every time. Steps 1 and 2 cost a minute together and
they are the ones that get skipped.

1. **`sailor memory page`**, and read the file it names. It is what this tree
   remembers, and it is rendered, not written by hand.
2. **`docs/decisions.md`** — the constraints that do not reopen without a
   measurement. If what you are about to write contradicts one, you have found
   a decision to reopen, not a line to write.
3. **`docs/gates.md`** — the whole list of checks, by letter. Run the letters
   that apply to what you touched. **It is the only list**: nobody assembles a
   shorter one for their own change. The list is complete in both directions,
   and two judges keep it so:

   ```sh
   cargo test -p sailor -j 1 --test every_declared_check_names_a_test_that_exists \
     --test every_ratchet_is_named_in_the_gates
   ```
4. **Break on purpose the thing you are proving**, and watch the outcome
   change. A check that stays green when you delete the line you are claiming
   checks nothing. This has caught real, shipped blindness five times in this
   tree, and reading never caught any of them.
5. **Report the measurement**, not the intention: the `test result:` lines from
   the tree under review, and an explicit verdict on what is closed and what is
   still open.

## What this tree refuses, and why it is mechanical

Every rule below is held by something that runs. The rules are here; the
commands that run them are in `docs/gates.md`, and that separation is
deliberate — two copies of a command list drift, and the one you read is never
the one the trunk runs.

### The product names nothing of the workbench it was built on

This is the oldest constraint here and the one most often broken by accident,
because the cheapest place to put a name you need *today* is always the code
in front of you.

- **No engine.** A provider introduces itself with a descriptor. Not in a
  sentence the product says, and not in the name of a symbol (ADR-001, held by
  `no_engine_is_named_in_the_code`).
- **A capability nobody declared triggers nothing, and absence is written
  down.** The product never guesses a default that happens to be right on this
  machine; it names what is missing and stops (ADR-013).
- **No forge, no remote, no trunk, no account.** Each is a fact about one
  repository, declared by that repository: in a descriptor, in
  `.sailor/delivery-policy.json`, or in a `sailor.*` key of its own git
  configuration, as `sailor.trunk`, `sailor.forgeAs` and `sailor.pushAs`
  already are (ADR-020, held by
  `no_forge_no_remote_no_trunk_is_named_in_the_code`).
- **No tool of the workbench.** An index, an editor, a protocol server, a
  plugin or a skill used while building Sailor is declared in a descriptor, not
  compiled into it: the index this repository uses is named in
  `.sailor/index.json`, and a repository that names none reads no pin and has
  its trees named by their path. Somebody downloading Sailor to run their own
  flows gets the product, not the toolbox of whoever wrote it (ADR-021, held by
  `no_workbench_tool_is_named_in_the_code`, **whose seed is 0**).
- **No home of another product** is a constant here (held by
  `no_product_home_is_written_into_the_code`).

The two seeds each of those judges carries may only fall. A name that is a fact
in a descriptor is right; the same name as a `const` in `crates/` is the debt
being counted.

**Where a repository declares its own facts**, so nothing has to be guessed:
`.sailor/delivery-policy.json` (what may merge, push, release, and on which
forge and remote), `.sailor/index.json` (the names its code index uses),
`.sailor/tools.d/*.json` (the tools it offers), and the `sailor.*` keys of its
git configuration — `sailor.trunk`, `sailor.forgeAs`, `sailor.pushAs`,
`sailor.indexServer`. What none of these declares, the product names as missing
and stops over. **It never falls back to a value that happens to be right
here**, which is the whole of ADR-013 applied outside engines.

The four in one line, which is how they are run:

```sh
cargo test -p sailor -j 1 --test no_engine_is_named_in_the_code \
  --test no_forge_no_remote_no_trunk_is_named_in_the_code \
  --test no_workbench_tool_is_named_in_the_code \
  --test no_product_home_is_written_into_the_code
```

### A source file holds code, not its own suite

A test module is **declared**, not written, in the file it tests:
`#[cfg(test)] mod tests;` beside a `tests.rs`. ADR-022, whose seed may only
fall. The ceiling on a file's length is a separate judge, which weighs product
lines apart from judge lines — and the two fall together,
because a suite moved out shortens the file it leaves.

```sh
cargo test -p sailor -j 1 --test a_source_file_holds_code_not_a_suite \
  --test files_do_not_grow_out_of_scale
```

### The repository carries the product, not the material that led to it

- **A page stays in `docs/` only if something that runs opens it by name**: a
  judge, a shipped flow, a compiled constant, or a contract both halves of the
  code are written against. Six do. Anything else — an essay, a walkthrough, a
  study, a proposal not in force — is working material.
- **Working material lives in Sailor's own store.** `sailor notes import` takes
  a markdown file in, `sailor notes list` shows what is held, `sailor notes show
  <slug>` reads it back byte for byte. Nothing is lost by taking it out of the
  tree; it stops being published, which is the point.
- **No mock-ups, no renders, no prototypes.** They were 27 tracked files and
  seven of every ten bytes in the tree. A picture of a window is not a source,
  and `npm run screenshots` writes under `target/`, which nothing tracks.
- **A document that leaves takes its links with it.** A dead link in a public
  repository is worse than no link, and a judge that opens a file by name
  panics when it is gone: both are found by running the battery, not by
  reading.

### Nothing of this machine is published

The repository is public and what lands here is world-readable, permanently.
No absolute path out of a developer's home, no private name, no profile home,
no credential, no flow of a person's own. Held by
`nothing_from_this_machine_is_published`, `nothing_reserved_is_tracked`,
`the_publication_boundary_holds` and `no_push_publishes_a_private_name` — and
the last of those refuses **before** the push, because a name cannot be
unpublished: a forced push leaves the commit reachable by its number.

## How it is verified — for code, the only oracle is `cargo`

**Never declare a thing done without evidence measured in the same turn.** And a
measurement is worth something only if it could have come out otherwise: break
on purpose the thing you are proving and watch the outcome change. For what a
person sees, the rendered screen is the oracle (ADR-003).

Traps already paid for:

- **Never pipe `cargo test` into `grep` or `tail`**: the exit code becomes that
  of the last command, and a red battery passes for green. Write the output to a
  file and read it.
- **Always `--no-fail-fast`, and it is not a detail of convenience.** Without
  it, `cargo test` stops at the **first red binary** and everything that comes
  after **is not run** — it does not fail: it does not start. A sandbox that
  denies `openpty` makes `crates/terminal` fall, and whoever types `cargo test`
  there is looking at part of the tree believing they are looking at all of it.
  It is recognised only by counting the binaries, not the tests.
- **A second build directory lives INSIDE `target/`, never beside it.** The
  `.gitignore` asks for `target-<something>` and it solved git's problem; the
  disk's problem it created. `cargo clean` empties **only** `target/`, so every
  `target-int`, `target-i18n`, and worse every `sailor-target-*` sibling of the
  repo, stays there for ever and nobody sees it grow. Call it `target/int`,
  `target/verifica`, as `release_cmd.rs:171` already does with
  `target/from-head`: one line of `.gitignore` covers it and `cargo clean` takes
  it back.
- **Every tree that is measured wants its own build directory.** A test binary
  carries the path of the tree that compiled it, so a worktree borrowing
  another's `target/` measures the wrong tree. Run with
  `CARGO_TARGET_DIR=$PWD/target/own`.
- **The seeds of the ratchets are measured on a clean `HEAD`, not on the working
  tree.** Somebody else's uncommitted files falsify every count, and seeds
  measured over them describe a tree that does not exist at `HEAD`. The right
  measurement is one command, and it costs a minute:

  ```sh
  sailor ratchet                 # every judge that reads the sources
  sailor ratchet --only comments_do_not_crowd_out_the_code
  ```

  It redoes `git archive HEAD` into `target/ratchet-tree` of the main checkout,
  shared with every worktree together with its build, lays over it the
  modified files and the new ones that land in a directory `HEAD` knows about
  (it lists them: look whether one of them is not yours), and prints of the red
  judges only what they said. It holds before saying «the tree is green» too:
  `git status` shows who else is writing, and their reds are not yours.
- **Never `cargo fmt` here.** `cargo fmt -- <file>` does not confine itself to
  that file: it formats the whole workspace, 262 files, and no gate in this tree
  asks for it. The style judge measures blank lines, not formatting.
- **`cargo test --tests` does not update the binary** the hooks run.
- **Compilation can be denied** when swap is high: use `-j 1`, and if it denies
  again **do not declare proved what you have not compiled**.
- **Heavy work takes the machine's turn, one at a time.** A whole battery, a
  workspace clippy, a release build: run it as
  `sailor machine turn -- cargo test --workspace --no-fail-fast`, and it waits,
  saying for whom, until whatever heavy work asked first is done. A flow step
  declared `"weight": "heavy"`, `sailor release` and `sailor ratchet` take the
  turn themselves. On 22/09/2026 three suites started together, each sized its
  compilers on the memory it saw free, and swap filled: nothing else stops that.

## How it is written

- **Everything is in English.** Identifiers, comments, documentation, commit
  messages, and every message a user of the tool can see. There is no inside
  language and no outside language: this repository is public, and what is
  committed here is world-readable, permanently. The rule is ADR-007 in
  `docs/decisions.md`.

  Identifiers include function names, types, fields, options, **local
  variables**, **modules**, **constants**, **file and directory names**, **CSS
  classes** and **JSON keys**. In Rust a file *is* a module: its name is an
  identifier like any other. That list used to be shorter, and the difference
  cost 136 renames — an incomplete rule is not a partial rule, it is a
  permission. The measure is `cargo test -p sailor --test
  identifiers_are_in_english`, which reads workflow job keys too.

  **Fixture data is data, not language.** `f.name == "assente"` stays as it is;
  the variable holding it is called `absent`.

- **Few comments, and no chronicle.** A comment says *why*, not *what*: the
  what is said by the code. If what you want to write can be had by renaming a
  variable, extracting a function or writing a test, do that instead. A comment
  earns its place when a *why* is left that the code cannot carry: an external
  constraint, a counter-intuitive choice, a declared limit.

  **Dates, "it used to do X", the story of how it went: not here.** They belong
  in the project's own records and in the commit message, which keep them with
  the real author and the real date instead of a hand copy. In the code, at most
  a pointer: `// see fault 39`.

  **Cap: six lines per block.** The ordinary comment is already inside the cap;
  what it hits is the tail of long narrative blocks, not the habit. Shortening
  is not deleting: a long block usually holds decisions already recorded
  elsewhere plus one limit that belongs next to the function causing it.

  The measure is `cargo test -p sailor --test comments_do_not_crowd_out_the_code`,
  and its numbers can only go down.

- **A product name in prose is counted, not forbidden.** `cargo test -p sailor
  --test a_product_name_in_prose_only_ever_falls`: a product name is a fact in a
  descriptor, the vocabulary of the ban inside a gate, and in prose it is either
  a measurement of this machine or this work sold against somebody else's — no
  rule of place separates those, and only a person can. So the mentions prose
  already carries are counted, the number may only fall, and a new one is red
  until somebody reads it and decides which of the two it is.

- **Flow and step `id`s stay as they are**, and the `.flow.json` filenames with
  them. This is not an exception to the language rule: **what the compiler
  reads is language, what the ledger keeps is data.** Renaming a step would
  make already-recorded runs show up as unknown steps, and renaming a shipped
  flow would silently stop a user's own replacement from winning. Recorded as
  ADR-008 in `docs/decisions.md`.

- **Commit messages: Conventional Commits.** `<type>(<scope>): <subject>`,
  lowercase, imperative, no trailing period. The body explains why, not what —
  it is where the chronicle the code must not carry actually belongs. No
  tooling attribution trailers.
- A comment that states something false is corrected at once. The code is the
  source; comments and documents are dated clues.

## The life of a branch, and what it is called

- **A branch is called `work/<what-it-does>`, in English, like the commits**:
  `work/terminal-claims`, not `work/annunci-terminali`; `work/toml-graft`, not
  `work/innesto-toml`. The name says the job, not the day nor the gesture
  (`fusione`, `ricucito`, `sera` are not jobs).
- **It is born from the trunk the repository declares, it returns there, and it
  dies.** The trunk is `git config --get sailor.trunk`, never a constant: this
  repository says `main`, and another repository says something else without a
  change to Sailor (ADR-020). Once the branch is merged, it is proved by content
  that it is superseded — merge it into a throwaway copy of the trunk and check
  the tree does not change, and first put through the same measurement a branch
  that must come out as carrying. Before deleting it, `git tag archive/<name>
  <sha>` and push the tag: from there the deletion is reversible, and the proof
  that it is, is `git for-each-ref --contains <sha> refs/tags refs/remotes`. The
  working copy is closed with `sailor worktree close <name>`, which asks who is
  standing in it first, **never with `git worktree remove` by hand**.
- **A tree is closed by whoever opened it, and by nobody else.** A tree you did
  not cut is not yours to take down, however clean and merged it reads: a
  workspace another program opened (anything under `~/orca/workspaces/`) holds a
  person's tabs, and the program closes them all when the directory goes. On
  21/09/2026 a session took one down with `git worktree remove` after a clean
  `lsof` reading, and a person's live session went with it: the session had been
  started with `cd <another tree> && claude`, so no process stood in the tree. Name
  it and leave it. The register says who opened it: `sailor worktree create`
  writes the tree down as it cuts it, and the flow that closes finished work
  takes down only a tree it finds written there, so one cut with `git worktree
  add` is named at the end and never taken down. **A reading taken inside the
  sandbox is not a reading**: `lsof` and `ps` there cannot see other processes
  and answer empty, which is «I could not look», never «nobody is here».
- **The shape of the name has a judge, and the judge is pure.** There are three
  shapes and nothing else: the trunk; `work/<what-it-does>` with lowercase,
  digits and hyphens in the topic; `worktree-agent-<id>`, which is written by
  the mechanism and nobody chooses. The proof is `cargo test -p sailor --test
  a_branch_is_named_for_the_work_it_carries`, the refusal happens where a branch
  is born (`workspace::create` asks `branches::may_be_cut`), and whoever wants
  the verdict on this tree types `sailor worktree names`.

  The judge reads a table of names written inside the test, **never the branches
  of this machine**: a check that goes red because somebody else left a branch of
  theirs standing gives a verdict on the machine, not on the work, and whoever
  receives it can do nothing about it. The command, which does read the real
  branches, is the other half. And the judge looks at the shape only: that
  `work/fusione-sera` tells of an evening instead of a job is said by the line
  above, and no comparison can say it.

## The trunk is reached only through a merged request

- **Nothing is pushed to the trunk.** A branch reaches it when its pull request
  is merged on the forge, and the merge commit is the proof. Measured on
  19/09/2026: every commit on the trunk had one parent, so no work had ever
  passed through a request — the three requests marked merged were closed by a
  push, all three at the same second. The forge held a mirror of the work, not
  the road it travelled.
- **The forge refuses the shortcut, so nobody has to remember it.** The trunk's
  ruleset requires a pull request and six green checks: `publication boundary`,
  `sailor/private-names`, `workspace tests`, `desktop tests`, `clippy gate`,
  `the style debt`. Each of the six has been proved able to refuse, on a real
  commit, with the receipt of the run: the table is at the foot of
  `docs/gates.md`, and `scripts/gates-can-say-no.sh` checks it still holds.
- **`sailor/private-names` is posted from here, not by the forge.** This machine
  is the only one holding the list, so `scripts/attest-private-names.sh <rev>`
  runs on the exact commit under review. Until it does, the request cannot merge
  and nothing says why.
- **Every call to the forge runs as the account the tree declares** in
  `sailor.forgeAs`, never as whoever happens to be logged in. A branch pushed
  from another account is a branch nobody watching this repository sees.
- **A branch with nothing open on it is invisible.** Work that is paused gets a
  draft request saying what it holds and what it needs, so the decision is made
  on a page instead of in a branch list.
- **The loop is run as flows, not by hand.** `docs/the-delivery-loop.md` names
  the flow for each step, from `open-the-draft-pull-request` to `cut-a-release`;
  `sailor flow list` says how often each has run. A step done by hand is a step
  whose flow nobody learns is broken.
- **In a shared checkout, commit by path and never restore the tree.** Several
  sessions write in the same working copy: `git add -A` sweeps somebody else's
  index, and `--abort`, `stash` or `reset --hard` throws away work that is not
  yours. Use `git commit -- <paths>`, and `--quit` where you would have written
  `--abort`.

## Whoever creates does not judge

The verdict on a piece of work goes to a context that did not produce it. If you
wrote the correction yourself, it is not you who declares it good: you report
what you wrote, how you proved it, and what remains uncertain.

## What leaves this tree carries no scaffolding

A commit message and a request to merge are **not addressed to us**. They are
read by strangers, for years, on a public repository, and they are the only
part of the work most people will ever see. They say what changed and why, in
the terms of the code and the product.

They do not name the sessions, the agents, the models or how the work was
divided among them; they do not say who reviewed whom; they do not name the
person whose decision it was. «A second opinion from a reviewer», «the owner's
decision», «found by <a session's codename>» are the shape to avoid — a real
request does not carry them, and a reader who was not here cannot use them.

The positive form is also the more useful one: **credit a finding to the thing
that found it, not to whoever was holding it**. «Caught by
`no_shipped_flow_names_a_skill_it_does_not_declare`» is an address the reader
can go to; «caught by the second reviewer» is an anecdote. When no such thing
exists, the finding stands on its own evidence, with no author at all.

**The story is not deleted, it is addressed elsewhere.** The project's own
records keep who did what on purpose, and that is their job. Sending it there is
deliberately a rule about destination and not about wording: deciding which
internal detail is harmless is a judgement call, and a judgement call made a
hundred times comes out wrong at least once.

## How it is reported

Short sentences, one idea per sentence, active verb. **The result first**, the
detail after only if it changes a decision. The thing first, then the mechanism:
a file name is an address, not an explanation.

Close with an explicit verdict: what is closed **with the measured evidence**,
and what remains open.

<!-- sailor:memories -->
What Sailor remembers in this tree is rendered by `sailor memory page`: run it, read the file it names, and do that before you reach for a tool of your own.
The rules of this tree, and the tools it hands you instead, are written in AGENTS.md.
<!-- /sailor:memories -->
