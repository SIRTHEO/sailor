# Sailor — instructions for whoever works in this tree

## What we are building

Sailor launches the command lines (Claude Code, Codex, Gemini), applies **one
single body of rules to all of them**, and runs every piece of work as a
**recorded flow** instead of a script, a hook or a binary of its own.

**Before fixing anything at all, read `docs/decisions.md`** — the permanent
constraints and the choices that do not reopen — and
[`CONTRIBUTING.md`](CONTRIBUTING.md), which lists every gate a change must pass.

## The decisions already taken, which do not reopen without a measurement

- **A flow is a data file; the nodes are actions registered in Rust.** No
  interpreter inside Sailor. The format is `{ id, description, graph, inputs }`:
  `graph` is what `flow::Graph` already loads and validates, `inputs` becomes
  the `root_inputs` of the request. The examples the product ships live in
  `crates/flow/system/`.
- **The name of the action is in the graph, the code is not** (`graph.rs`, above
  the `action` field). A new step is a Rust action registered in
  `crates/actions`, never a script.
- **The brake sits at the boundary of the process**, not in the native hooks of
  a single command line. The command lines stay untouched: Sailor reads their
  configuration and migrates it.

## How it is verified — for code, the only oracle is `cargo`

**Never declare a thing done without evidence measured in the same turn.** And a
measurement is worth something only if it could have come out otherwise: **break
on purpose the thing you are proving** and watch the outcome change. If the
check stays green when you remove the line you are claiming, the check checks
nothing. For what a person sees, the rendered screen is the oracle (ADR-003).

Traps already paid for:

- **Never pipe `cargo test` into `grep` or `tail`**: the exit code becomes that
  of the last command, and a red battery passes for green. Write the output to a
  file and read it.
- **A second build directory lives INSIDE `target/`, never beside it.** The
  `.gitignore` asks for `target-<something>` and it solved git's problem; the
  disk's problem it created. `cargo clean` empties **only** `target/`, so every
  `target-int`, `target-i18n`, and worse every `sailor-target-*` sibling of the
  repo, stays there for ever and nobody sees it grow. Call it `target/int`, `target/verifica`, as
  `release_cmd.rs:171` already does with `target/from-head`: one line of
  `.gitignore` covers it and `cargo clean` takes it back.
- **Always `--no-fail-fast`, and it is not a detail of convenience.** Without
  it, `cargo test` stops at the **first red binary** and everything that comes
  after **is not run** — it does not fail: it does not start. A sandbox that
  denies `openpty` makes `crates/terminal` fall, and whoever types `cargo test`
  there is looking at part of the tree believing they are looking at all of it.
  It is recognised only by counting the binaries, not the tests.
- **The seeds of the ratchets are measured on a clean `HEAD`, not on the working
  tree.** Somebody else's uncommitted files falsify every count, and seeds
  measured over them describe a tree that does not exist at `HEAD`. The right
  measurement is one command, and it costs a minute:

  ```sh
  sailor ratchet                 # every judge that reads the sources
  sailor ratchet --only comments_do_not_crowd_out_the_code
  ```

  It redoes `git archive HEAD` into `target/ratchet-tree`, lays over it the
  modified files and the new ones that land in a directory `HEAD` knows about
  (it lists them: look whether one of them is not yours), and prints of the red
  judges only what they said. The rite by hand — archive, `cp` file by file,
  `cargo test --manifest-path` with `CARGO_TARGET_DIR=$PWD/target/from-head` —
  remains the explanation of what it does, no longer the gesture. It holds
  before saying «the tree is green» too: `git status` shows who else is writing,
  and their reds are not yours.
- **`cargo fmt -- <file>` does not confine itself to that file**: it formats the
  whole workspace. The tree is not formatted wholesale and is not to be
  formatted wholesale.
- **`cargo test --tests` does not update the binary** the hooks run.
- **Compilation can be denied** when swap is high: use `-j 1`, and if it denies
  again **do not declare proved what you have not compiled**.

## How it is written

- **Everything is in English.** Identifiers, comments, documentation, commit
  messages, and every message a user of the tool can see. There is no inside
  language and no outside language: this repository is public, and what is
  committed here is world-readable, permanently.

  This is the rule the project was founded with, and it was lost. It lived in a
  `CLAUDE.md` on an orphan branch with an unrelated history — one commit, never
  published, unreachable from anything. The project then spent days
  rediscovering it piece by piece. The branch is kept as the tag
  `archivio-primo-abbozzo`; its rule is ADR-007 in `docs/decisions.md`.
  It is the most expensive shape of the defect this project keeps chasing: not
  a rule nobody interrogates, but **a rule nobody could read**.

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
  in the fault ledger and in the commit message, which keep them with the real
  author and the real date instead of a hand copy. In the code, at most a
  pointer: `// see fault 39`.

  **Cap: six lines per block.** Measured 2026-09-01: 3,036 blocks, median 3
  lines — the ordinary comment is already inside the cap and nothing changes
  for it. What overflows is 636 blocks carrying two thirds of the 14,343
  comment lines, the longest being 66 consecutive lines. The cap hits the tail,
  not the habit. Shortening is not deleting: the first block trimmed was that
  66-line one in `flow/src/subflow.rs`, five decisions already recorded
  elsewhere plus one limit that now sits next to the function causing it.

  The measure is `cargo test -p sailor --test comments_do_not_crowd_out_the_code`,
  and its numbers can only go down.

  **The measure is `cargo test -p sailor --test
  a_product_name_in_prose_only_ever_falls`**, and it counts rather than
  forbids. A product name is a fact in a descriptor, the vocabulary of the ban
  inside a gate, and in prose it is either a measurement of this machine or
  this work sold against somebody else's — no rule of place separates those,
  and only a person can. So the mentions prose already carries are counted, the
  number may only fall, and a new one is red until somebody reads it and
  decides which of the two it is. Measured on a clean `HEAD`, never on the
  working tree: several sessions write in this checkout.
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
- **It is born from `main`, it returns into `main`, and it dies.** Once the
  branch is merged, it is proved by content that it is superseded: merge it into
  a throwaway copy of the trunk and check the tree does not change, and first
  put through the same measurement a branch that must come out as carrying.
  Deleting the branch is a separate decision, asked of whoever owns it, as
  CONTRIBUTING says; the working copy is removed with `git worktree remove`.
**The shape of the name has a judge, and the judge is pure.** There are three
shapes and nothing else: `main`, the trunk; `work/<what-it-does>` with
lowercase, digits and hyphens in the topic; `worktree-agent-<id>`, which is
written by the mechanism and nobody chooses. The proof is `cargo test -p sailor
--test a_branch_is_named_for_the_work_it_carries`, and whoever wants the verdict
on this tree types `sailor worktree names`, which exits with a code other than
zero and prints the names outside the convention.

The judge reads a table of names written inside the test, **never the branches
of this machine**: a check that goes red because somebody else left a branch of
theirs standing gives a verdict on the machine, not on the work, and whoever
receives it can do nothing about it. The command, which does read the real
branches, is the other half. And the judge looks at the shape only: that
`work/fusione-sera` tells of an evening instead of a job is said by the line
above, and no comparison can say it.

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
person whose decision it was. «A second opinion from codex», «the owner's decision»,
«found by <a session's codename>» are the shape to avoid — a real request does
not carry them, and a reader who was not here cannot use them.

The positive form is also the more useful one: **credit a finding to the thing
that found it, not to whoever was holding it**. «Caught by
`no_shipped_flow_names_a_skill_it_does_not_declare`» is an address the reader
can go to; «caught by the second reviewer» is an anecdote. When no such thing
exists, the finding stands on its own evidence, with no author at all.

**The story is not deleted, it is addressed elsewhere.** The project's own
records keep who did what on purpose, and that is their job. Sending it there is deliberately a rule about destination and not about
wording: deciding which internal detail is harmless is a judgement call, and a
judgement call made a hundred times comes out wrong at least once.

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
