# Everything Sailor can do, and how much of it shows

**02/09/2026.** A systematic survey of the engine, crate by crate, measured by
interrogating the code and the binary. Born from a sentence of the owner's in front
of a preview: *«you are doing maybe 1% of the product»*. He was right, and this
document is the reckoning.

**How to read it.** Every entry has a state:

| | |
|---|---|
| **CLI** | there is a command that does it |
| **window** | there is a surface that shows it or commands it |
| **nobody** | the engine can do it and nobody can ask for it |

Where a number appears, it comes from a command anybody can run again. Three
throwaway tools turned out blind during this survey and their numbers were
thrown away; they are listed at the end.

---

## 1. The nineteen crates, and what each one exposes

| crate | lines | what it can do | does it show? |
|---|---:|---|---|
| `actions` | 12,133 | the 23 actions a step can run | **partial** — the window offers 7 families |
| `sailor` | 18,760 | the command line: 14 commands, 49 verbs | **CLI** |
| `flow` | 8,021 | the graph, the runs, the scheduling, the spending ceilings, the subflows | **partial** |
| `toolbox` | 5,510 | which tools are there, in what version, whether you are authenticated | **nobody** |
| `ledger` | 5,591 | the store: runs, steps, events, calls, inventory | **partial** — the history only |
| `terminal` | 3,557 | real pseudo-terminals, with routing of the requests | **window** |
| `models` | 3,005 | the price list, the prices, the quota left, the modes | **nobody** |
| `ui` | 2,330 | what the window reads: views, gathering, sources | **window** |
| `inventory` | 1,771 | skills, agents, commands, rules, hooks — and which are switched off | **CLI** |
| `sessions` | 1,635 | who comes in, what happens, what is on the machine | **CLI** |
| `supervisor` | 1,384 | rebuilding and putting back into service hot | **nobody** |
| `trigger` | 849 | the triggers: what makes a run start | **partial** |
| `registry` | 845 | the registry of the actions and the link to the store | internal |
| `faults` | 794 | the faults: what broke and the check that is missing | **CLI** |
| `profiles` | 697 | the profiles of a command line, with their homes | **CLI + window** since 02/09 |
| `relay` | 511 | the hand-over between sessions | **nobody** |
| `catalogue` | 410 | the language of what gets read | internal |
| `release` | 320 | putting into service a binary built from HEAD | **nobody** |
| `workspace` | 162 | the worktrees of a repository | **window** |

**Six crates out of nineteen have no door at all**: `toolbox`, `models`,
`supervisor`, `profiles`, `relay`, `release`. That is 11,447 lines of engine
nobody can ask for anywhere except by reading the code.

---

## 2. The twenty-three actions a step can run

Extracted from the `*_ACTION` constants. The window's box offers **seven
families**, each with a single default action: the other sixteen are reachable
only by writing the JSON by hand.

| action | what it does | in the box? |
|---|---|---|
| `shell_check` | runs a command and says whether it went | **yes** (check) |
| `external_engine` | calls a command-line engine | **yes** (engine) |
| `handed_to_agent` | hands the step to a person | **yes** (human) |
| `subflow` | runs another flow as a step | **yes** (subflow) |
| `store_write` | writes into the key-value store | **yes** (deposit) |
| `trigger` | the entry point of the flow | **yes** (trigger) |
| `type_into_terminal` | types into a terminal Sailor owns | **yes** (gesture) |
| `store_read` | reads a key from the store | no |
| `store_list` | lists the keys of a prefix | no |
| `history_ask` | interrogates the history of the runs | no |
| `mcp_ask` | calls a tool of an MCP server | no |
| `mcp_ready` | checks that an MCP server answers | no |
| `detect_tools` | surveys what is on the machine | no |
| `tool_needs` | crosses what is needed with what is there | no |
| `fault_record` | records a fault | no |
| `fault_list` | lists the faults | no |
| `work_claim` | takes on a shared piece of work | no |
| `work_release` | releases it | no |
| `work_survey` | looks at who is doing what | no |
| `take_mandate` | takes the mandate of a session | no |
| `ask_without_interaction` | asks without waiting for a person | no |
| `empty_terminal` | checks that a terminal is idle | no |
| `measure_terminal` | measures what is in a terminal | no |

**The three shared-work actions** — `work_claim`, `work_release`,
`work_survey` — are the coordination between agents, and in the window there is
nothing that names them.

---

## 3. The fourteen commands, and their forty-nine verbs

| command | verbs | in the window |
|---|---|---|
| `flow` | list · run · check · cost · cap · due · relocate · resume · schedule · tick | **2 out of 10**: list, run |
| `session` | list · open · close · attach · detach · census · event · install | **0 out of 8** |
| `faults` | list · add · check · import · render · status | **0 out of 6** |
| `terminal` | list · run · press · reset · mandate | **4 out of 5** — `mandate` is missing |
| `profiles` | list · current · create · switch | **4 out of 4** ✓ since 02/09 — `current` is the «in force» row |
| `models` | list · current · set | **0 out of 3** |
| `worktree` | create · list · remove | **3 out of 3** ✓ |
| `step` | open · close | **0 out of 2** |
| `workspace` | init | **0 out of 1** |
| `inventory` | — | shown in part |
| `remaining` | — | **no** |
| `release` | — | **no** |
| `run` | — | **no** |
| `version` | — | **no** |

**Eighteen verbs out of forty-nine have a door.** Thirty-one do not.

---

## 4. The product concepts, one by one

### 4.1 The workspace — the biggest hole

A workspace is declared with **`sailor.json`** in the root of the project
(`crates/flow/src/workspace.rs`). The file may be `{}`: what counts is **where
it sits**, because its position is the answer to the question «what is the
root». Inside it can declare:

| field | what it says |
|---|---|
| `name` | what the project is called |
| `rules[]` | which documents count as rules (AGENTS.md, decisions.md…) |
| `checks{}` | the checks of the project, by name |
| `equipment` | what equipment it demands |
| *(unknown fields)* | kept, never a reason to reject — it is fault 8 |

And the origin of a flow carries the warning with it: `this project` if the
marker is there, **`this project (no sailor.json: root guessed)`** if the root
was guessed by walking up to a `flows/` directory.

**What is missing, and it is everything:**

- there is **no list of workspaces**: Sailor knows one at a time, the one it is
  running in;
- you cannot **move from one project to another** from the window;
- **the flows of a project** show up mixed in with those of the home, told apart
  only by the origin label;
- `sailor workspace init` exists and is **the only verb**: there is no `list`,
  no `switch`, no way of seeing what a project has declared;
- the 31/08 document on credentials asks the real question and nobody has solved
  it yet: *«I would hate it if another terminal opened in other workspaces could
  have other credentials»*.

### 4.2 The profiles and the credentials

`crates/profiles` can: list them, say which is in use, create them, swap the
homes with a symbolic link (`SymlinkSwap`), build the environment of a child
process (`build_environment`), recognise the known CLIs (`KnownCli`).

Measured on this machine: **two profiles, both `NOT AUTHENTICATED`**. You find
out when a run fails. In the window: nothing.

> **Opened on 02/09/2026.** The window now has the «Profiles» screen: the known
> command lines with **the note saying how we know it**, the profiles of each
> with the access state asked of the engine inside *that* profile's home, and
> the two gestures — switching to a profile, creating one. The answer is the one
> from `sailor profiles list`, not a second one: `profiles_cmd::overview()` is
> the single copy. What stays out is the workspace credentials and the
> `env_clear()` of the paragraph below.

The plan written on 31/08 goes further and is not built: **global** credentials
and credentials **of a workspace that never leave it**; the environment of the
child that **gets built instead of inherited** (`env_clear()`); authorisation as
a triple (workspace × action × profile).

### 4.3 The tools and the capabilities — `toolbox`, 5,510 lines, zero doors

It can say, for every tool: whether it is there, where, in what version, whether
you are authenticated (`LoginStatus`), which capabilities it offers
(`Capability`, `CapabilityForm`, `CapabilityState`), which hooks and abilities a
session has (`SessionHooks`, `SessionAbilities`), and what a flow is missing to
run (`ToolNeedsAction`).

The flow `what-this-machine-has` does exactly this round and prints «this flow
does not run because this is missing, and it is installed like so». **In the
window none of it exists.**

### 4.4 The triggers — two shapes, and one is declared and mute

`trigger::Kind` has **two variants**: `Manual` and `Terminal`. The descriptors
are data (`~/.config/sailor/triggers.d/`), the shape is code.

`Terminal` **is declared and does not listen**. It is written in the repository
because it is a choice: *«a simulated listening would be worse than an absent
one, because a green flow would say somebody had spoken»*.

**Time** is missing entirely: `flow` already has `Schedule`, `Recurrence`,
`is_due`, `tick` — and the document `time-is-the-last-choice.md` lists the five
decisions to take before its node gets written.

### 4.5 The store — eight tables, and **this paragraph said something false**

`runs` · `steps` · `events` · `model_calls` · `inventory_items` · `processes` ·
`snapshots` · `store`.

On this machine the store **exists and is alive**: 8 MB in
`~/.claude/state/flussi`, written today. This paragraph said «never created»
because it had looked in `~/.config/sailor/ledger` — and `default_directory`
looks **first** in the home of whoever came before, which is where it sits. A
survey that looks at one path alone finds what that path knows.
What is not shown is the events, the model calls, the inventory over time, the
processes, the key-value store.

The August mandate was already saying it: *«Sailor records everything that
happens and never goes back to read it»*.

### 4.6 The models, the prices, the quota

`models` can: the price list (48 entries measured), which one is in use,
changing it, the prices per million tokens, the modes accepted
(text/image/video), the context, and **how much quota is left** read from the
engine (`Remaining`, `from_claude_oauth_usage`).

In the window: **none of this**. A run stops on the limit and nobody knows how
much was left — which is the fact the 29/08 research was born from.

### 4.7 The hand-over, the supervision, the release

- **`relay`** — passing the baton between sessions. The design is in the note
  `2026-08-28-il-flusso-che-accompagna`. No door.
- **`supervisor`** — `cargo_build`, `rebuild_then_swap`, `LiveStatus`,
  `close_the_ones_that_stopped_breathing`: rebuilding and putting back into
  service hot. No door. The command `live_status` is exposed by the shell **and
  the window does not call it**.
- **`release`** — `Readiness`, `Service`, `Target`, `read_stamp`: putting into
  service a binary built from HEAD, never from the working tree. No door.

### 4.8 The sessions

`sessions` can: who comes in and who goes out (`Arrival`, `Inhabitant`), the
census (`Census`), the events of a terminal, the anchoring by tty
(`tty_of_nearest_ancestor`), and the measure of how full a session is
(`Fullness`). Eight verbs from the command line, **none in the window**.

### 4.9 The faults

`faults` can record them, list them, check them, import them, render them, and
say the standing of each (`Standing`). Measured **66** in
`docs/faults-encountered.md`. In the window: nothing.

---

## 5. What the window actually calls

Twenty-four commands out of the twenty-six the shell exposes:

`day_summary` · `delete_flow` · `discover_tools` · `execution_history` ·
`flow_trigger` · `flows` · `known_runs` · `machine_inventory` · `manual` ·
`open_runs` · `run_snapshot` · `run_usage` · `save_flow` · `start_run` ·
`step_history` · `terminal_*` (6) · `worktree_*` (3)

Never called: **`flow_places`** (where the flows sit on disk) and
**`live_status`** (the live state, which the window polls instead of listening
to).

---

## 6. The list of what is to be connected, in order

The order is by **how much engine it unlocks per line of window written**.

### First — what is needed every day and is not there

1. ~~**The workspaces**~~ — **done on 02/09**: the registry
   (`workspaces.json`), `sailor workspace list` with its `gone`, the «Projects»
   screen and the flows grouped by origin. What remains is the **move** from one
   project to another, which shifts root, terminals and credentials together.
2. ~~**The profiles**~~ — **done on 02/09**: which is in use, the access state
   asked of the engine inside each one's home, moving from one to another and
   creating them. What remains is what the 31/08 plan calls credentials **of a
   workspace**, and the `env_clear()`.
3. ~~**The quota and the prices**~~ — **done on 02/09**: how much was
   **consumed** (never «left»: on a window with no ceiling the subtraction is an
   invention), the price list with the prices per million, and the model **in
   force** told apart from the configured one — the free-engines-only rule
   overrides silently, and a screen showing the wish alone would explain
   nothing.
4. ~~**The tools**~~ — **done on 02/09**: what is there, where, in what version,
   **where it looked**, and the rows of the list that cannot be read. The bridge
   was flattening three states into a boolean: «it is not there» and «I could
   not look» now arrive told apart. What stays out is the capabilities
   (`Capability`) and `ToolNeedsAction`.

### Second — the store that gets interrogated

5. ~~**The eight tables**~~ — **done on 02/09**: the processes the store saw
   start and never finish (**with the pid asked for now**, because an open
   record is not a live process), the runs never closed, how the last fifty
   broke **by class**, what the flows have deposited, and the inventory over
   time.
6. ~~**The faults**~~ — **done on 02/09**: the register, the check each one is
   missing, and the **fourth state** no yes-or-no can say: a formula the
   register does not know is not «closed», or a fault would drop out of the
   count through a rewrite nobody meant that way.

### Third — the actions nobody can compose

7. ~~**The actions**~~ — **done on 02/09**, but only halfway: the window
   **lists all twenty-three of them**, asked of the live registry and grouped by
   family, and names the ones the canvas cannot draw instead of slipping them in
   among the checks. Composing them stays the canvas's work; here what shows is
   **what exists**, which is the question that had no answer.
8. **The shared work**: `work_claim`, `work_release`, `work_survey` — seen in
   the list, never staged.

### Fourth — what has to be decided before building it

9. **Time**: five open decisions, written on 01/09.
10. **The trigger from a terminal**: declared and mute, by choice.
11. **The credentials per workspace**: the 31/08 plan, never built.
12. **The hand-over** and **the hot rebuild**: two whole engines with no door.
    The hand-over, though, **is not a screen**: it is four actions
    (`measure_terminal`, `type_into_terminal`, `empty_terminal`,
    `take_mandate`) and their door is that a flow can use them — which now
    shows, under «What Sailor can do». The supervisor the window already uses
    (`live::watch`); the release is a gesture that touches the binary in
    service, and exposing it is a decision, not a wiring job.

---

## Appendix: the three tools that turned out blind

1. **The grep over the action registrations** answered `alfa`, `zeta`, `spia` —
   names from tests. The real actions sit in the `*_ACTION` constants, and there
   are 23 of them.
2. **The grep over the enum variants** gave zero on `StepSpecies`,
   `trigger::Kind` and `CapabilityForm`, all three of which exist. The right
   form is to search the uses (`StepSpecies::`), not the declaration.
3. **The grep over the fields of a struct** gave zero on `Declaration`, which
   has five fields. `grep -A30` is not enough when the documentation sits
   between one field and the next: you read the line index.

The rule already written holds: when a list comes out empty, before concluding
«it is not there» you give the tool a case that **must** come out positive.
