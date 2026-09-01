# Sailor

Sailor makes the command-line agents you already have — Claude Code, Codex,
Gemini CLI, anything else — work together inside **flows** you can read,
measure and stop.

It is not another agent. It is the scaffolding around the ones you use: it
decides which engine to call, under which identity, how much it may spend, and
it **writes down what actually happened** — so a run can be reviewed, compared
and repeated instead of recounted.

> **Status: under construction, and used every day by the people writing it.**
> Interfaces change. What is here works and is tested; what is missing is
> written down in `docs/guasti-incontrati.md`, open defects included.

## What it does, concretely

- **Runs flows**: a graph of steps, where independent ones genuinely start
  together. A step can call an engine, run a check, read and write a store, or
  **hand the work to a person**.
- **It knows no engine by name.** Every tool introduces itself with a
  *descriptor* declaring how you talk to it: how to ask a one-shot question,
  which words it uses to refuse a malformed line, how it says it is out of
  quota, how to ask whether it is authenticated. What does not declare
  triggers nothing — **staying silent is different from guessing.**
- **Tries the command lines before spending**: `sailor flow check` assembles
  each engine's real command line and runs it *without the question*, so a
  malformed line is found there and not on the first paid run.
- **Measures what it costs**: tokens per class, cost per call, a spending cap
  that tightens as the remainder falls. And when part of the bill is not
  measured **it says so in place of the number**, instead of handing you a
  bare figure you would read as the total.
- **Knows which identity every engine started under**: separate credential
  profiles per command line, and every call records which home it used and
  **how that home was chosen** — never an empty field.
- **Leaves no orphan processes**: it records what it starts, and can stop it
  from a different invocation later.

## Building and testing

```sh
cargo build --workspace
cargo test --workspace      # around 930 tests across 79 targets
```

Rust 1.89 or newer. No service and no database to start: the store is a SQLite
file created on first use.

The desktop window (Tauri + React) lives in `desktop/` and is **outside the
workspace**:

```sh
cd desktop && npm install && npm run live
```

`npm run live` starts `sailor-live`, not `cargo tauri dev`. The difference is
not taste: `cargo tauri dev` closes the window on **every** touched file,
*before* compiling, so a compile error makes it vanish and it does not come
back. `sailor-live` builds first and touches what is running **only if** the
build succeeded: the window survives, changes its title and shows the error.
The long version is fault 11 in `docs/guasti-incontrati.md`.

## The commands

| command | what it is for |
|---|---|
| `sailor flow list \| check \| run \| cost \| cap` | flows: which exist, whether they hold up, running them, what they cost, what cap they carry |
| `sailor step open \| close` | the steps a live agent takes charge of |
| `sailor run <cli>` | starts a command line with its profile's equipment |
| `sailor profiles list \| create \| switch` | each engine's identities, and whether they are authenticated |
| `sailor remaining` | how much quota is left, read from the provider rather than inferred |
| `sailor inventory` | what this machine can do, and what has appeared or disappeared |
| `sailor session` | the tracked terminals and what is running right now |
| `sailor workspace` | the project root and what it declares |

`sailor --help` lists them all, and each command explains its own forms.

## How it is built

Fifteen Rust crates in one workspace, with the boundaries placed where the
responsibilities divide rather than where it was convenient:

| | |
|---|---|
| `flow` | the engine: graph, parallel fronts, spending cap, resume |
| `actions` | what a step can do, and how an external engine is invoked |
| `toolbox` | the descriptors: how to talk to a tool we do not know |
| `ledger` | the store: events, and the projections derived from them |
| `models` | catalogue, price list, remaining quota |
| `profiles` | the credential homes, one per command line |
| `registry` · `trigger` · `release` | the action registry, triggers, releases |
| `inventory` · `sessions` · `supervisor` · `terminal` | the machine: what exists, what runs, what is live |
| `ui` · `sailor` | the shared view and the command line |

## The rules this project holds itself to

Few, and they count for more than style preferences. In full in
[`AGENTS.md`](AGENTS.md) and [`docs/decisioni.md`](docs/decisioni.md).

- **A test counts only if it can come out differently.** It is written before
  the repair, watched being born red, and verified by putting the **original**
  defect back. A test never seen to fail is not a test.
- **What is not a measurement does not become a zero.** An unknown cost is
  unknown, not zero; an empty list because nobody could look is not "there is
  nothing there". When the error happens, it must go in the direction that
  worries — never in the one that reassures.
- **Whoever makes a thing does not judge it.** The verdict on a piece of work
  does not come from whoever wrote it.
- **A rule in a comment is not a defence: it is the shape of one.** It counts
  only where a list applies it or a check interrogates it.
- **Every defect gets written down**, in `docs/guasti-incontrati.md`, with how
  it came to light and **what would have stopped it** — because what follows a
  fault is a check, not a task assigned to somebody.
- **Identifiers in English. What a stranger reads — this README, the CI, the
  messages a user of the tool sees — in English. Comments inside the code, and
  everything under `docs/`, in Italian.** Decided by Theo on 2026-09-01, so
  expect the fault log and the decision record linked above to be in Italian:
  they are the workshop's notebooks, not the product's surface.

## Licence

[GNU AGPL v3](LICENSE).

You may use it, modify it, and sell it. If you modify it — or offer it as a
network service — **you must publish your changes** under the same licence.
Nobody can take this work and close it.
