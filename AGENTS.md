# Sailor — instructions for whoever works in this tree

## What we are building

Sailor launches the command lines (Claude Code, Codex, Gemini), applies **one
single body of rules to all of them**, and runs every piece of work as a
**recorded flow** instead of a script, a hook or a binary of its own.

The measure of every job: *does this thing take an approval away from Theo, or
bring him a better doubt?* If it does neither, it is not work.

**Before fixing anything at all, read `docs/decisions.md`** — the permanent
constraints and the choices that do not reopen — **and the `da-fare` note**,
which says where we stand and what is about to disappear: `sailor notes show
da-fare`. Repairing a piece that has to disappear is work against the plan, and
no local check shows it: everything stays green.

The decisions are in the repo on purpose, the working notes in Sailor's store.
This line once pointed at a document a cleanup had already deleted: for two days
the first instruction of every session was an empty address, and nobody noticed,
because a broken pointer in a document is not red.

## The order of the work, decided by Theo

**Code in Sailor → debt removal → building the flow.** Never the other way
round. You do not build inside what has to disappear.

## The decisions already taken, which do not reopen without a measurement

- **A flow is a data file; the nodes are actions registered in Rust.** No
  interpreter inside Sailor. The format is `{ id, description, graph, inputs }`:
  `graph` is what `flow::Graph` already loads and validates, `inputs` becomes
  the `root_inputs` of the request. The first example:
  `flows/passa-il-testimone.flow.json`.
- **The name of the action is in the graph, the code is not** (`graph.rs`, above
  the `action` field). A new step is a Rust action registered in
  `crates/actions`, never a script.
- **The brake sits at the boundary of the process**, not in the native hooks of
  a single command line. The command lines stay untouched: Sailor reads their
  configuration and migrates it.

## How it is verified — the only oracle is `cargo`

**Never declare a thing done without evidence measured in the same turn.** And a
measurement is worth something only if it could have come out otherwise: **break
on purpose the thing you are proving** and watch the outcome change. If the
check stays green when you remove the line you are claiming, the check checks
nothing.

Traps already paid for on this machine:

- **Never pipe `cargo test` into `grep` or `tail`**: the exit code becomes that
  of the last command, and a red battery passes for green. Write the output to a
  file and read it.
- **A second build directory lives INSIDE `target/`, never beside it.** The
  `.gitignore` asks for `target-<something>` and it solved git's problem; the
  disk's problem it created. `cargo clean` empties **only** `target/`, so every
  `target-int`, `target-i18n`, and worse every `sailor-target-*` sibling of the
  repo, stays there for ever and nobody sees it grow. On 2026-09-04 it was
  **27 GB across four directories**, 3.9 GB of them in a `target-i18n` that no
  file of the tree named. Call it `target/int`, `target/verifica`, as
  `release_cmd.rs:171` already does with `target/from-head`: one line of
  `.gitignore` covers it and `cargo clean` takes it back.
- **Always `--no-fail-fast`, and it is not a detail of convenience.** Without
  it, `cargo test` stops at the **first red binary** and everything that comes
  after **is not run** — it does not fail: it does not start. Measured on
  2026-09-01 inside the perimeter, where the sandbox denies `openpty` and
  `crates/terminal` always falls. The binaries that do not start are always the
  same ones, the tail of the alphabet — `toolbox`, `trigger`, `ui` and seven
  integration tests. Whoever types `cargo test` in there is looking at three
  quarters of the tree believing they are looking at all of it.

  **The figures of this paragraph were «36 out of 47» and they were out of
  date.** Re-measured on 2026-09-04 with `--no-fail-fast` and the output on a
  file: **108 binaries plus 19 doc-targets, 1,252 tests, 1,199 green, 53 red —
  and all 53 of them are the sandbox** (43 for `mkdir /tmp/sr-*` denied, 11 for
  `openpty` denied). **Zero truly red.** In CI on 2026-09-02: 109 binaries,
  1,134 tests, 2 truly red. All 19 crates have tests. A number written here and
  not re-measured is a false guard like the others: whoever reads «36 out of 47»
  today concludes that a quarter of the tree is missing, and it is not.

  That day cost a piece of work declared finished with a regression inside it:
  the test that was falling was in `toolbox`, and the `grep FAILED` of whoever
  went looking for it could not find it because that test had **never started**.
  It is the same family as the line above — a green outcome that looked at
  nothing — and it is recognised only by counting the binaries, not the tests.
- **The seeds of the ratchets are measured on a clean `HEAD`, not on the working
  tree.** More than one session writes in this checkout, and another's
  uncommitted files falsify every count: on 2026-09-04 I wrote seeds measured
  with another session's uncommitted test in the tree — its Italian comments
  raised one counter, its lines of code lowered a crate's ratio — and the seeds
  described a tree that does not exist at `HEAD`. `sailor release`, which runs
  the suite on a clone of `HEAD`, stopped without replacing anything, and that
  is how it was seen. The right measurement is one command, and it costs a
  minute:

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
  `archivio-primo-abbozzo`; the rest of what it said is in `docs/decisions.md`.
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
  66-line one in `flow/src/subflow.rs`, five decisions that were already in
  `docs/decisions.md` plus one limit that now sits next to the function causing
  it.

  **And the reason is not taste.** The semantic index does not strip comments:
  SocratiCode's `chunkFileContent` embeds the text as written, and calls
  "preamble" everything preceding a declaration. A 66-line block **becomes the
  chunk the index compares against your question**, instead of the code below
  it. A project that is 25% narrative comment hands you the story when you ask
  it for the code.

  The measure is `cargo test -p sailor --test comments_do_not_crowd_out_the_code`,
  and its numbers can only go down.

- **Notes that cannot meet these rules stay out of the repository**, under
  `~/personal/.sailor-notes/`, which has no git remote and nothing in it is
  ever copied in. That covers absolute paths from a developer machine, client
  or employer names, internal repository names, transcripts and logs copied out
  of private tooling, and any framing of this work as a reaction to or
  comparison with somebody else's product.

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
  flow would silently stop a user's own replacement from winning. Decided by
  Theo on 2026-08-31, in full in `docs/decisions.md`.

- **Commit messages: Conventional Commits.** `<type>(<scope>): <subject>`,
  lowercase, imperative, no trailing period. The body explains why, not what —
  it is where the chronicle the code must not carry actually belongs. No
  tooling attribution trailers.
- **Writes only with the file-editing tools**, never with `sed`, a heredoc or an
  interpreter script: writes from an interpreter skip the checks of this house.
- **Absolute paths**, never `cd X && command`.
- A comment that states something false is corrected at once. The code is the
  source; comments and documents are dated clues.

## The working copies: whoever opens one, closes it

On 2026-09-02 **53 abandoned ones were found, 39 GB**, born in 28 hours in
bursts of six to eight an hour. All clean, no process inside, and **50 out of 53
already inside the trunk byte for byte**: it was not lost work, it was clutter.

Nobody removed them because it was believed that removing them deleted the
branch. **That is not true, and it is measured**: `git worktree remove
<directory>` removes the directory and leaves the reference where it is. Orca's
command is another thing. So closing a copy costs nothing and nothing is lost.

- **When you have finished, remove your copy.** `git worktree remove` on the
  directory, and the branch stays consultable.
- **Do not remove somebody else's** without measuring first: `git status
  --porcelain` inside, and no process with its `cwd` there. If either of the two
  speaks, ask.
- **Deleting the branch is another decision**, and it is not yours: it is proved
  first that the content is already in the trunk, and it is asked.

**The first gesture in a copy is to look at where it was cut from.** It is not
prudence, it is fault 101: on the evening of 2026-09-05 five copies out of six
were born **1120 commits behind**, on the old line of `main`, where `sailor
ratchet` does not exist and neither do the files of the seeds. An agent worked
in one for a whole turn and delivered a commit that could not be merged,
declaring seeds that were not this tree's. The comparison costs a second:

```sh
git log --oneline -1 && git log --oneline -1 main
```

If they do not match, `git reset --hard main` before reading any file at all.
A copy cut from a reference nobody chose is not isolation: it is another project
with the same name.

## Integration has one branch only

Eleven of those 53 existed **only to merge** — `fusione-quattro`, `fusione-sei`,
`fusione-46`, `fusione-sera`, and so on — and the trunk carries **47 merges for
some forty work branches**. Every session that finished opened its own copy to
integrate and redid the same conflicts from scratch. That is where the tokens
went: not writing the same code twice, but merging it twelve times in twelve
places.

Do not open a branch to merge. Merge your work where it is already integrated,
and if you do not know where that is, **ask the neighbours before opening one**.

## How it is proved that a branch is superseded

The ancestor is not enough and sometimes lies: after a rewrite of the history or
a squash, `merge-base --is-ancestor` says «no» about work that is already all
there. **The content is compared**: merge the branch into a throwaway copy of
the trunk and look at whether the tree changes.

And before believing the result, **the absurd check**: put through the same
measurement a branch that *must* come out as carrying — one with a file in it
that the trunk does not have. If it comes out «superseded», the measurement is
blind and every number of that pass is thrown away.

## The life of a branch, and what it is called

Measured on 2026-09-05: **64 local branches, 56 with content identical to the
trunk** and five working copies open for days, all clean and all already inside.
The trunk was **196 commits ahead of the remote** and nobody had pushed. Names
like `work/fusione-sera-guasti` and `innesto-toml-codex-ricucito`: they tell of
an evening, not of a job.

- **The trunk is `main`**, and it is the only branch: it is pushed to
  `origin/main` **at every release**, not «when we remember», because a release
  that puts into service a binary the remote has never seen is a release that
  exists on one machine only. The history before the rewrite of 2026-09-06 is
  the tag `archive/before-the-rewrite`: a tag is not to be mistaken for a place
  where the work continues.
- **A branch is called `work/<what-it-does>`, in English, like the commits**:
  `work/terminal-claims`, not `work/annunci-terminali`; `work/toml-graft`, not
  `work/innesto-toml`. The name says the job, not the day nor the gesture
  (`fusione`, `ricucito`, `sera` are not jobs).
- **It is born from `main`, it returns into `main`, and it dies.** Once the
  branch is merged, it is proved by content that it is superseded (below) and it
  is deleted in the same gesture; the working copy is removed with `git worktree
  remove`. A branch that survives its own merge is clutter somebody will have to
  re-measure.
- **Whoever finds somebody else's branch does not delete it**: they prove it by
  content, and if it is superseded they say so to whoever opened it — or to
  Theo — with the measurement alongside.
- **There are two names you do not choose, and they have to be closed all the
  same.** An agent in a working copy is born on a branch the mechanism names by
  itself; a step that asks for a tree of its own opens a copy under the name of
  the run and of the step. Neither of the two deletes itself: whoever merges
  closes the first, whoever reads the work closes the second. Measured on
  2026-09-05: twelve orphan copies and 905 MB after an evening of delegations,
  and it is fault 89.
- **The trunk is pushed at every release, and if the remote refuses it, you look
  at why.** A machine with more than one access to that remote has to say which
  one owns the repository: `sailor.pushAs` and `sailor.pushSecretFrom` in the
  configuration of this tree. On 2026-09-05 the remote stayed 179 commits behind
  for a day because the active access was not the owner, and every release said
  so honestly while nothing moved.

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

## How this tree is explored before it is changed

**Before searching by hand, the index is asked.** SocratiCode has this
repository indexed and answers by symbol, by dependency graph and by semantic
search: `codebase_symbol` for where a thing lives, `codebase_search` for «who
does X», `codebase_impact` for who you touch by changing it. A `grep` on a tree
of forty thousand lines finds the occurrences, not the relations, and whoever
develops without asking the index redoes by hand a measurement that is already
there.

**The limit is declared**: the index lags behind files a few hours old, and on
Rust the graph has given false orphans (fault 38). It holds as a first question,
not as a verdict: what the index says is confirmed by reading the file it names.

**And there is already a measurement of what not asking costs**: `sailor search
<words>` searches among the flows, the runs, the store, the events and the
faults of this machine, and answers questions no reading of the code can — how
many times a step has failed, which fault has already been written, what
yesterday's run learned.

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
person whose decision it was. «A second opinion from codex», «Theo's decision»,
«found by <a session's codename>» are the shape to avoid — a real request does
not carry them, and a reader who was not here cannot use them.

The positive form is also the more useful one: **credit a finding to the thing
that found it, not to whoever was holding it**. «Caught by
`no_shipped_flow_names_a_skill_it_does_not_declare`» is an address the reader
can go to; «caught by the second reviewer» is an anecdote. When no such thing
exists, the finding stands on its own evidence, with no author at all.

**The story is not deleted, it is addressed elsewhere.** The ledger, `sailor
notes` and the fault register record who did what on purpose, and that is their
job. Sending it there is deliberately a rule about destination and not about
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
