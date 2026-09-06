# The decisions

**This file is the memory of the choices, and the flows read it.** It is not a
diary: every entry is a decision that binds future work, with who took it and
why. A flow about to choose what to do, or about to implement something,
consults it **first** — otherwise it takes up a road already discarded, and
nobody notices until it is written.

**Why it exists.** On the night of 29/08/2026 seven decisions were taken. They
existed nowhere except in the commit messages and in the conversation they were
born in: the flow launched the next day could not know them. This is the defect
that separates a system that learns from one that starts over.

**A decision gets written here when it binds somebody who was not present.** If
it concerns only whoever took it and ends with them, it is not a decision: it is
a work choice, and it belongs in the commit.

## The permanent constraints

They are not decisions taken once: they are the yardstick every other one is
judged by. A proposal that violates them is discarded, even when it is better in
every other respect.

| constraint | what it means in practice |
|---|---|
| **Independence from the model** | Sailor works with any command-line tool, including those that do not exist yet. A solution that works only on one precise engine has to be **declared as a capability** of that tool, and whoever does not have it has to go on working while paying more. |
| **Clarity for whoever is looking** | Sailor exists so that a person may see and control what their tools do. An optimisation that makes how the steps pass information to each other opaque is **worse than the cost it saves**. It holds for the look as well: an interface that hides what is happening is the opposite of the product. |
| **The screen is the judge** | A project rule that cannot be checked by looking at an image is an opinion. It comes from the two defects that neither the types nor the tests saw. |
| **Whoever creates does not judge** | The verdict on a piece of work is given by whoever did not write it. An engine that checks itself already has its own conclusions in context: it is not distracted, it is compromised. |
| **A test counts only if it could have come out differently** | After writing it you break on purpose the thing it tests, and watch it go red. Whoever declares they did not do this is turned away. |
| **We write in code only what touches the world** | The engine that runs, the store that records, the gate that authorises. All the rest is a flow, changeable without recompiling. The boundary is **the power**, not «runs versus decides». |

## The decisions taken

### English everywhere, restoring the charter the project was founded with

**2026-09-01**, decided by Theo.

Identifiers, comments, documentation, commit messages, and every message a user
of the tool can see are in **English**. There is no inside language and no
outside language: the repository is public, and what is committed here is
world-readable permanently.

**This is not a new rule. It is one that was lost.** It lived in a `CLAUDE.md`
on an orphan branch with an unrelated history — one commit, never published,
reachable from nothing. It also said: no absolute paths from a developer
machine, no employer or client names, no internal repository names, no
transcripts or logs copied out of private tooling, no framing of this work as a
reaction to or comparison with somebody else's product — and it named
`~/personal/.sailor-notes/`, a directory with no git remote, as where notes that
cannot meet those rules belong. The branch is kept as the tag
`archivio-primo-abbozzo`.

**Why it matters more than its content.** This project spent days
rediscovering, one incident at a time, things that were written in its first
commit. The morning of the same day a partial version of this very rule was
decided again from scratch — English for what a stranger reads, Italian inside
— by looking at a CI file. That is the most expensive shape of the defect the
project keeps chasing: not a rule nobody interrogates, but **a rule nobody
could read**. A rule on an unreachable branch is worse than a missing one,
because everybody assumes the ground was covered.

**What does not change.** Flow and step ids, and the `.flow.json` filenames,
stay as they are — see the 2026-08-31 entry. That is not an exception to the
language rule: what the compiler reads is language, what the ledger keeps is
data.

**How it gets done, and the order.** Prune first, translate after. The six-line
comment cap already requires 636 blocks to shrink or go, and those carry two
thirds of the comment volume: translating a comment that should be deleted pays
for the same line twice. The measure is
`cargo test -p sailor --test comments_do_not_crowd_out_the_code`, whose Italian
count — 11,854 lines the day of the decision — can only go down. This file, and
the rest of `docs/`, convert as they are touched.

### The language is chosen by who reads: English what a stranger sees, Italian what whoever works here sees

**01/09/2026**, decided by Theo while looking at the CI.

The `README`, the CI files and **the messages a user of the tool sees** — what
`sailor` prints, what it says when it refuses, the `--help` texts — go in
**English**. Comments inside the code, test messages and everything under
`docs/` stay in **Italian**.

**What changes from before.** `AGENTS.md` said «comments and messages in
Italian», in a single line, without telling the two kinds of message apart. The
boundary that was there — «what the compiler reads is in English, what the store
keeps is data» — divided the code well and had nothing to say about the shop
window: as long as the repository was private, the shop window did not exist.
The day `main` became Sailor the question arose on its own.

**The occasion.** The CI file, written on 31/08, had three jobs called `prove`,
`stile` and `finestra`: keys read by `needs:`, by the GitHub APIs and by
`gh run`, that is, identifiers, on a page anybody can open. The rule about
identifiers was already there; what was missing was somebody to interrogate it
about the `.yml` files, because `identifiers_are_in_english` read the Rust
sources only. It now reads the job keys as well.

**The boundary is who reads, not what kind of file it is.** A `panic!` inside a
test speaks to whoever works here: Italian. The same `panic!` on a path a user
can walk speaks to them: English. The ambiguous case gets asked about, not
settled by picking the convenient language.

**What it does NOT touch.** The `id`s of the flows and of the steps and the
names of the `.flow.json` files **stay in Italian**: they are data the store
keeps, and the decision of 31/08/2026 that protects them still holds — renaming
a step would make the runs already recorded show up as unknown steps. Whoever
reads this entry and thinks the flows ought to follow the shop window should ask
first.

### References are resolved in one place only, after the condition; and `input` is what the step received

**01/09/2026**, from fault 28.

How a step receives the work of the step before **is not a choice of the
individual action**: it is the semantics of the graph, and it lives in
`flow::step_input` — the single point every step of every run passes through. An
action does not resolve its own references, it receives them already resolved,
as it already receives the `workdir` resolved. Every registered action inherits
it, including the ones nobody has written yet.

**The order inside that point is the decision, and it is not an implementation
detail**: compose the dependencies with the `with`, resolve the `workdir`,
evaluate the `when`, and resolve the references **only if the step runs**. *Why
the `when` first*: a skipped step receives the input maimed by the dependency
that is not there, so its pointers find nothing — and a step that does not run
must not break over work it will not do. Measured on
`flows/chiedi-all-indice.flow.json`, which with the opposite order went from
«completed» to «ended with status failed». *The price, declared*: a `workdir`
written as `{"$from": …}` does not get attached to the root. It was already so,
and it counts for less than the case above.

**What this forces on whoever writes a check.** A behaviour test stays green
with a copy of the resolution put back inside an action: the behaviour does not
change, only the number of places the rule lives in — which is the fault. So the
guard **counts the places**
(`crates/sailor/tests/references_are_resolved_in_one_place.rs`), and has in its
turn tests that interrogate it: twice that reader went blind in silence, and a
check that can switch itself off is not a check.

**And what `StepRecord::input` means, which this decision changed.** One rule
only: *the input as the step received it at the moment it stopped being
processed.* Resolved if the step runs; **not** resolved if it was skipped or if
it broke precisely while resolving a reference — in the second case on purpose,
because whoever reads has to see the pointer to be corrected and not the void
that came out of it. **Which of the three it is is not said by that field: it is
said by `outcome`, in the same record.** It has to be written here because a
`{"$from": …}` read in a record is not in itself a defect — on `Skipped` it is
the norm, on `Broke` it is the diagnosis, on `Went` it would be a fault — and
whoever reads the store without this line would read the three things the same
way.

**The residue, measured and not hidden**: a run started before 01/09 and resumed
after compares an old raw fingerprint with a new resolved one, and declares
`DifferentInput` on the same work. It is a wrong label on an attempt, not lost
data, and it dies out on its own as those runs finish.

### A step can be handed to the live agent; the judgement cannot
**31/08/2026.** A step can declare the action `handed_to_agent`: it describes
the work and **starts nothing**. It is run by the agent already alive in the
terminal, who then comes back into the system with `sailor step open` and
`sailor step close`. The record of the step stays what it always was — intention
written before, outcome written after — and the run does not notice who was in
between.

**Why, with the measurement.** A four-step flow costs **2.79 times** a single
prompt on the same task, and the ratio of the consumption **is the ratio of the
turns**: 62 against 30. It does not read more per turn (+8%): it does twice the
turns, because every step starts a process that rediscovers the repository from
scratch. Fattening the passage between the steps would make things worse; the
cure is not to reopen a conversation that is already open.

**What this does not concede, and it is the point.** Handing over the execution
is not handing over the verdict. Whoever closed a step **may not open or close**
a step that depends on it: it is the permanent constraint «whoever creates does
not judge» applied to the gesture the judgement gets written in. The refusal
holds at both points on purpose — at the opening alone it would be got round by
opening under one name and closing under another.

**Denial is the default, not a list of permissions.** A flow that really wants
the same hand declares it step by step with `"same_holder_ok": true`. The
direction matters: a forgotten list of permissions lets everything through and
nobody notices; a forgotten denial at worst stops a piece of work, and shows up
at once.

**Whoever holds a handed-over step is a deadline, not a process.** `held_by_pid`
stays empty and nobody asks the operating system anything: it is fault 12, where
`pgrep` inside the perimeter answered empty *without an error*. The resumption
(`sailor flow resume`) compares `handoff_timeout_secs` with `started_at`; what
it cannot see — a record with a pid, or with no readable deadline — **it does
not declare dead**.

**Two declared weaknesses, written in the code and not only here.** (1) `--as
<who>` is a name whoever writes it picks for themselves: Sailor has no session
identifier to read, so the refusal above holds against carelessness, not against
somebody who wants to get round it. (2) On a flow with hand-overs the **spending
ceiling stops being a guarantee**, because the agent's consumption is
self-declared (`sailor step close --turns`). This is why that row carries
`cost_micros` empty and not an estimated number: that way it goes into
`Spend::calls_without_cost`, `is_complete()` becomes false, and every place that
shows the ceiling already says the real spend is higher. An invented cost would
make *complete* a sum that is not.

### The language: identifiers in English, all the rest in Italian
**31/08/2026.** Everything the compiler reads is in English — functions, types,
fields, variables, modules, constants, **file names**, CSS classes, JSON keys.
Everything a person reads is in Italian: comments, error messages, text in the
window, documents, and the **data** of the tests.
**Why it is here and not only in `AGENTS.md`.** It was only there, and on 31/08
136 violations were counted — nearly all written in the three preceding days, by
sessions that had received «answer in Italian» as a strong instruction and this
line as one among many in a document. This file is the memory that gets read
again before correcting anything: if a rule is not here, it is not binding in
fact, whatever it says elsewhere.
**And above all it has a measurement.** `cargo test -p sailor --test
identifiers_are_in_english` looks for Italian words in declaring position, and
knows the file names too. It is not an analyser: it is a list of words, which
has no false positives and lets through the ones it does not know. The price is
declared; the alternative was to go on measuring nothing.
**The lesson, which holds beyond the language.** A rule no check interrogates
never goes red — it is the same defect as the dead pointer `AGENTS.md` tells
about itself, and as fault 22, where a zero never computed passed for a
measurement. Whoever writes a new rule writes what makes it red as well.

### The identifiers of the flows and of the steps stay in Italian
**31/08/2026 — Theo.** `sviluppa-sailor`, `verdetto`, `implementa`, the names of
the `.flow.json` files: they stay as they are. The boundary is not between code
and data in the abstract — it is this: **what the compiler reads is in English;
what the store keeps is data, and data does not get renamed for style.**
**Why**, with the two consequences no compiler takes. (1) The store has runs
already recorded with those `step_id`s: a `verdetto` step turned into `verdict`
makes the old one look unknown and the new one look never run, and the
resumption after a crash no longer finds its own steps. (2) The decision «system
flows live inside the binary» says that whoever wants a different one writes one
**with the same name** in their own home, and theirs wins: changing the shipped
name would make a flow somebody has already written stop winning, **in
silence**.
**What follows.** The check `identifiers_are_in_english` does not look at the
`.flow.json` files and never will look at the `id`s: it is not an oversight to
be completed. Whoever extends it to the data in future is breaking this
decision, not applying it. The declared asymmetry stays:
`flows/dispatch-the-work.flow.json` has its id in Italian and its steps in
English, and that is fine — both are data.

### The spending ceiling belongs to the flow, and the width of the front follows from it
**31/08/2026.** A flow can declare `spend_cap_micros`: how much one of its runs
may spend. Before opening each front the executor asks the store how much has
been spent; if the ceiling is reached the run stops with a word of its own —
`cap_reached`, not `failed` — and says which steps did not start.
**Why before opening and not inside the action**: a step that finds out halfway
through that it has overshot has already paid. The only instant at which
stopping costs nothing is before opening the front.
**Why a word of its own and not a fault**: a nightly flow that touches its own
ceiling every night would look broken every night, and whoever is watching would
stop watching.
**What the ceiling does not promise**: it is measured on the costs the engines
declare. Codex declares the total of the tokens and not the two sides, so its
row stays without a cost and does not enter the count. The ceiling is a
guarantee **over what is known**, and the stopped run writes down how many calls
were outside — because whoever is about to raise it and launch again has to know
beforehand, not after.
**The default is no ceiling.** `None` is not `Some(0)`: the first is «nobody put
a limit», the second is «this flow must spend nothing». A ceiling that appeared
on its own would stop runs nobody asked to stop, and it would do it at night.

### A ceiling is not calibrated on fewer than three runs that cost something, and today none is calibrated
**31/08/2026.** `sailor flow cap <name>` suggests a value **only** with at least
three runs of that flow that spent something known. Below the threshold it
refuses to suggest and says what is there. The suggestion, when there is one, is
*worst run observed + dearest call observed*.

**Why three, and why the second addend.** With two samples the maximum and the
minimum are the only two values: calling the greater of two «worst observed» is
an invented figure with the face of a measurement, and it is fault 22 in another
shape. The second addend is not prudence: the check fires *before* opening a
front, never inside a call, so the run stops with the grain of a call and not of
a micro — the sum says «the dearest run I have seen, plus the grain I know how
to stop with».

**And today no flow reaches the threshold. Measured on the store of this
machine on 31/08/2026, read-only**: 34 runs, and **6 with a cost other than
zero** — `come-lo-risolvono-gli-altri` 2, `esamina-la-repo` 2, `prova-dei-turni`
1, `sviluppa-sailor` 1. The other 28 are fault 22, where the cost was the
constant zero until 30/08. **The discarded proposal was «median + 50%»**: on
that column the median would give zero for every flow, that is, a ceiling that
stops every run before the first step — and it would do it at night, with the
air of a calibration on many samples. Whoever wants to calibrate the ceilings
will do it when the samples are there, not before.

**The ceiling is not tied to `native_spend_cap`**, the capability declared by
claude-code alone: different scope (a run against an invocation), a different
word for stopping, and one engine out of four has it. Making the brake depend on
it would mean the ceiling holds or does not hold depending on who answers.

**And the figure is called «equivalent cost» wherever it is shown.** It stays in
micros of currency, but «spent 5.00 against a ceiling of 5.00» makes you believe
an invoice was stopped: with a local command line you pay a subscription, and
what gets consumed is quota. `sailor flow cost` said so already; `why_it_stopped`
did not, and the same number was read two ways depending on the command showing
it.

### The capabilities of a tool are data, and absence gets written down
**31/08/2026.** A descriptor declares, besides `detect`, `version`, `ask` and
`usage`, a **`capabilities`** block: what that engine can do beyond answering —
resume a session, branch it, impose a shape on the answer, isolate itself from
the configuration of its host, receive an equipment set, hold a spending ceiling
of its own, choose the model, fall back on another. It is a map from name to
declaration: **the code knows no capability name at all**, so adding one to a
new tool is writing a JSON file in `~/.config/sailor/tools.d/`, never
recompiling. The permanent constraint «we write in code only what touches the
world», applied to a vocabulary.

**Writing `false` is not the same as keeping quiet, and it is the point of the
whole block.** `false` says «somebody looked and it is not there»; the absence of
the line says «nobody looked». A block that only allowed listing what is there
would make every omission pass for measured — and it is the same distinction the
detection keeps between «it is not there» and «I could not look». This is why
the four shipped engines answer on **all nine** capabilities of the vocabulary,
and a test demands it.

**Whoever does not have it goes on working, and the fallback stays today's.** An
absent capability is not an error: whoever cannot impose a shape on the answer
has it asked for in the prompt with `answer_shape` and pays more tokens. The
permanent constraint «independence from the model». A step declares what it
needs with `needs_capabilities`, and `sailor flow check` **warns**, naming step,
engine and capability — it does not fail: a flow written for a more capable
engine is not broken, it is a flow that costs more here, and it is the same
reason a tool that is not installed is a warning and a name that does not exist
is an error.

**What this does not do, and must not seem to do.** The actions do not use any
capability yet: the vocabulary and the check that interrogates it exist, the use
does not. `needs_capabilities` is declared in `EngineSpec` so that an honest
step is not accused of a typo, and it is not read at execution.

### `flow check` runs: it assembles every command line and tries it without the question
**31/08/2026.** From fault 1 onwards the cure written next to every fault about
command lines is the same — «a test that really runs every command line before
it ends up in a flow» — and it stayed uncovered for three days, because running
seemed to mean spending. It does not. **An engine invoked with the real line and
without the question calls no provider, and walks the same argument parsing as a
real call**: if the line is malformed it says so right there, free. From today
`sailor flow check` assembles the line of every engine of every chain, runs it
without the question, and reports how it stands.

**The verdict is in the text, never in the exit code.** Measured on this
machine: `agy` exits **2** both when it refuses properly («flag needs an
argument: -print») and when the line is the malformed one of fault 27
(«--print took "--output-format" as its prompt»). A probe judging by the outcome
would have seen the two cases as identical and would have walked past fault 27 —
which is exactly what happened. This is why the descriptor declares
`ask.refuses_without_prompt`, **the engine's words**, as it already does for
`unusable_when`; and this is why `judge_dry_run` does not even receive the exit
code, so there is no way of using it by mistake.

**`--help` is the wrong harmless form.** `agy --mode nonsense-value
--not-a-real-flag --help` exits **0**: it short-circuits before reading the
arguments, so it approves an invalid value and an invented flag. The right form
is to assemble the real line and not give the question.

**It changes the nature of the command, and that has to be said.**
`resolver.rs` declares that resolving a name must run nothing, and it stays
true: it is the check that starts processes, not the resolution. `flow check` is
no longer only static — with no network, no money, with an explicit time ceiling,
because on this machine `timeout` and `gtimeout` do not exist.

**On by default, with `--no-engines` to switch it off.** A check behind a flag
is a check nobody interrogates: nobody would have written `--engines` to look
for a defect they did not know they had. Switched off, the report **keeps
quiet** instead of declaring healthy lines it did not look at — the same rule as
the absent detector.

**Five outcomes, five sentences, because they are five different repairs:**
healthy; broken (with the engine's words in full and the assembled line); not
tried (three distinct reasons: the descriptor is silent, the engine is not here,
no answer); not assemblable (no `ask` block); cannot work right now. And
`unusable_when` is read **before** `refuses_without_prompt`: an exhausted engine
is not a broken engine, and read the other way round it would send somebody off
to correct a healthy descriptor.

**What this does not say.** That an engine was **really called**: that is known
by the store, and it stays a separate axis. Mixing them would make an engine no
run has ever named pass for used — which is fault 32.

### The power of a step: the Bazel model, under observation
**29/08/2026 — Theo.** A step declares what it needs, and the rest does not
exist for it. The check comes in as a **warning** and becomes a barrier only
with a change of configuration, after it has been seen working.
**Why**: a specific ban gets got round, a restricted world does not; and the
observation phase takes away the fear that makes these things impossible to
introduce.
**What follows**: every step of the existing flows will have to declare what it
touches. It is not free. *Not built yet.*

### The authorisations file does not exist
**29/08/2026 — Theo.** Self-repair has no gate of its own: it is a flow like the
others, with the powers it declares.
**Why**: if the Bazel model holds for every step, a special mechanism for
self-repair would be defending the same thing twice. And it is consistent with
the fact that the flows we use to develop Sailor are not shipped to anybody.

### The system flows live inside the binary
**29/08/2026 — Theo.** Embedded at compile time, not installed as files next to
the program. Whoever wants a different one writes one with the same name in
their own home or in the project, and theirs wins.
**Why**: a flow shipped as a file can be missing, go stale or be deleted, and
then the product behaves differently on different machines without anybody
understanding why. *Done: `crates/flow/system/`.*

### No bridle on the flow that develops
**29/08/2026 — Theo.** The step that implements writes without asking
permission.
**Why**: the perimeter is not yet enforced by the engine, and waiting for it
would have stopped everything. Whoever launches knows this. **Careful**: in a
cycle this counts double — whoever lets it run alone for hours has to be able to
see what it does while it does it, and from this round on the text of a step
comes out on stderr while the step runs.

### Red tests break the step
**29/08/2026 — after the first failed round.** No tolerance on the step that
runs the tests in the development flow.
**Why**: the tolerance was there so that the verifier could see the outcome even
when they failed, and that way **a piece of work that did not compile got past
the gate** — with five minutes of verification spent on code that did not stand
up. A piece of work that does not compile has nothing for anybody to judge.

### Flows compose, they do not merge
**29/08/2026 — Theo.** Research, dispatch, development and interrogation of the
code are the phases of a single cycle, but they stay separate flows that call
each other.
**Why**: a ten-step flow that does everything cannot be used by halves, and the
research is useful on its own too. **What follows**: `subflow` is needed, a step
that runs another flow. *Not built yet.*

### The cycle lives inside Sailor, not beside it
**29/08/2026.** A patrolling flow is not a long flow: it is a short flow run
many times, and whoever runs it again has to be Sailor.
**Why**: a script that relaunches was written and deleted the same day. It would
have been a sticking plaster outside the system over a hole inside the system,
and sticking plasters stay. **What follows**: somebody is needed to run what
`sailor flow due` already computes. *Not built yet.*

### The text does not repeat numbers the system can give
**29/08/2026.** Where a fact is already recorded, the text points at it instead
of copying it.
**Why**: a copy made by hand goes stale on its own. It has already happened: a
document said «ten faults» while the file listed eleven, and a verifier rejected
a whole piece of research over that inconsistency — rightly.

### The unlocking order has changed: first use Sailor, then need nothing else
**31/08/2026 — Theo.** The order written on 29/08 — calls, orchestration, cycle
— stays valid as a technical sequence, but **it is no longer the criterion by
which what to do is chosen**. The new criterion is one only: *what is missing
for Theo to be able to spend a working day inside Sailor.* Three blocks, in this
order, and the third is the consequence of the first two:

1. **Sailor develops without dying while it is being used.** It has to be
   possible to fix the machine underneath while somebody is working on top of
   it: no restarts, no window vanishing. Today it is prevented by two open
   faults — **4** (Sailor does not know which processes it started, so it can
   neither stop them nor resume them) and **11** (in live mode a compile error
   in any crate at all kills the window instead of leaving it on the last good
   version).
2. **The terminals.** A terminal opens **bound to a workspace** — a repository,
   a project — and what the user types gets **routed**: if the request concerns
   a flow, it goes to the flow; otherwise it stays an ordinary terminal. Today
   none of it exists: `desktop/src-tauri` has four files and not one line of
   pseudo-terminal, and the trigger source `sailor-terminal` is declared in the
   catalogue as «the shape it will have, not a measurement».
3. **Needing nothing else**, which is not a piece of work of its own: it is what
   happens when the first two are done.

**Why this order and not the previous one.** The old order optimised the
correctness of the engine; this one optimises the moment the system stops being
a project and becomes the tool the work is done with. As long as Theo develops
Sailor elsewhere, every defect of Sailor's is paid for by somebody else — and
none of its faults gets found by using it, which is the only way the faults of
this repository have been found so far.

**What follows, and it has to be said because it changes the priorities of
whoever reads.** A piece of work that makes Sailor more correct but not more
*usable from inside* does not come before one that makes it usable. It holds for
the flows too: writing new ones is not in the first two blocks, and whoever
writes one while these three are open is working outside the order.

### The unlocking order: first the calls, then the orchestration, then the cycle
**29/08/2026 — Theo.** Three blocks, in this order, and each is seen working
before the next:

1. **The calls to the models**, profiles and providers together. Including the
   free quotas the providers declare and which we do not exploit today, and the
   command lines we do not have yet (DeepSeek, Grok, OpenRouter and the others).
2. **Orchestrating well**: sending the work to the right model for that work,
   and drawing flows that stand up.
3. **Fortifying the development flows**, running them in a cycle, and under a
   real dispatch chain that uses the machine instead of one step at a time —
   knowing whether the machine is busy with whoever is working on it or free.

**Why this order**: without the first block every run depends on a single
subscription and stops when it runs out, as happened on 29/08. Without the
second, having more engines only means having more ways of wasting. The third is
what makes the whole thing a system that goes on by itself, and it comes last
because until then every defect is multiplied by the number of runs.

**After these three**, the rest is improvement: the planned entries are
followed.

### Everything built as a flow has a flow that tends it
**29/08/2026 — Theo.** Self-repair and development are not a separate project:
they are the pair of flows that holds up everything we keep at the flow level.
**Why**: what is not code has neither a compiler nor tests watching over it. A
broken flow stays broken in silence until somebody launches it. If flows are the
place we put everything that does not touch the world — and it is the permanent
constraint at the top of this file — then their maintenance has to be just as
serious as the code's, and automatic for the same reason.

### An entry can be deprecated or decided again, and not on its own
**29/08/2026 — Theo.** While the flows are being developed, the work entries
change: some no longer make sense, others have to be rethought. **This is done
together with whoever uses the system, not autonomously.**
**Why**: an entry that disappears without anybody knowing is indistinguishable
from a forgotten entry, and the second is a fault. It holds the other way round
too: a flow that deletes on its own what looks superseded to it decides in place
of whoever has to decide — and it is the same reason the first rule of choice is
«never an entry waiting on a decision».
**What follows**: when the entries move into the store, the state is not
«open/closed». At least **deprecated** is needed — it is not done any more, and
it is written why — and **to be decided again**, which is an entry that is
waiting for you and that no flow may take. And the passage into those states has
to be recorded with who did it, like everything else in the store.

### Multi-provider is built at home, and is not a proxy
**30/08/2026 — Theo, after looking at free-claude-code.** Neither
`free-claude-code` nor any of the other intermediaries (Claude Code Router,
LiteLLM, OmniRoute, 9router) gets integrated. The piece is made here.

**Why, with the numbers that decided it.** That project is 143,000 lines of
Python 3.14 with 157 pinned packages and an always-on server, to be put under a
Rust workspace that keeps three dependencies by a choice written in the
`Cargo.toml`. It has 51,600 stars and **one single person** writing it. And
above all: **the piece it was wanted for is not inside it.** Its catalogue has
an identifier, a URL and the name of the environment variable — no quota, no
limit, nothing about what the provider does with the data it receives. The «over
1.3 billion free tokens a month» is **a README line with no figure behind it**.
The expensive piece is the translation of the formats between providers: twelve
thousand lines they rewrite two and a half times a quarter, and which would
become ours for ever.

**What gets taken anyway, and what gets refused.** To be refused without
discussion: two of their fifty providers present themselves as another program —
the OAuth client of the Codex CLI and its `User-Agent` — to pass an agent off on
somebody else's subscription. That is not getting round a quota, it is
pretending to be another piece of software, and it does not come in here. To be
taken, instead, **a piece of data that already exists and is under MIT**: the
catalogue of OmniRoute's free tiers, which is the only one of the four to carry
documented monthly quotas per provider, the methodology it measured them with,
and — the thing that is worth most — a verdict on the terms of use marking
seventeen providers as «to be avoided, their terms forbid going through an
intermediary». The dataset gets taken, not the program.

**The road this opens, and it costs almost nothing.** Sailor already launches
the agents as subprocesses with a configurable environment (`launch.env`), and
there are endpoints that speak **natively** the protocol those CLIs already use:
a variable gets pointed there, and there is no translation to write or to
maintain. The work that stays ours is the one nobody has done: a catalogue of
the providers carrying **how much they give free**, **on what pact about the
data**, and **how much is left**. It lives in `crates/models`, which already
keeps the models.

**What follows, and it is not built yet**: where the credentials live (today the
profiles move files and make symbolic links, that is, the secret is in the clear
on the disk); and the dimension that has to go into the routing rule from the
start — **not all jobs can go everywhere**, because on certain free tiers the
pact is that your data trains the model, and a flow that reads private code does
not have the same set of permitted destinations as one that summarises a public
document. Adding it later means having already sent something to the wrong
place.

### An action declares the surface it belongs to, and the powers it demands

**31/08/2026 — Theo**, after the survey of `dev-stack` (27 scripts of another
project, candidates to become flows).

The surfaces are four, and an action declares **one only**: `sense` reads the
world without touching it, `act` touches it, `remember` is the interrogable
store, `gate` is where a person's permission comes in. Along with the surface,
every action declares **the powers it demands** — network, disk, processes,
money, secrets — and the `sense` ones declare in addition **what they answer
when they cannot see**.

**Why it is not an aesthetic taxonomy.** The survey was looking for «which
scripts become flows» and found something else: 15 entries stuck on **five
missing powers**, not on fifteen nodes. The right question is not which node is
missing, it is **which power we do not have and which flow proves it**. And the
rule that follows from it is a single line: *if an orchestration calls for new
code, a power is missing — not a flow.*

**Why the third declaration exists.** It comes from fault 12: a command silenced
by the perimeter answered «empty» without an error, and the watch said «no flow
running» while two were running. A sensor that confuses zero with blind is worse
than an absent sensor, because whoever is downstream trusts it.

**What makes it red** — without this it would not be binding, like the rule
about the language before 31/08: a test that walks the action registry and fails
if one does not declare surface and powers, and if a `sense` does not declare
its own blind answer. **It is born red on all nine of today's actions.**

> **That test was never written, and it is fault 67, open.** Checked on
> 04/09/2026: searching for `surface` in the crates returns two files, and one
> of the two — `crates/actions/src/history.rs` — says so of itself, *«the four
> surfaces do not exist in the code»*. As long as the test is not there, this
> paragraph describes a rule nobody can violate, which by the rule above is a
> rule that is not there. The choice between writing the test and withdrawing
> the rule is Theo's, and it sits in fault 67: **this note does not take it, it
> makes it visible.**

**The debt, declared.** The seven building sites open on 31/08 (`supervisor`,
`terminal`, `presence`, `mcp`, and the others) produced crates **before** this
criterion. If they are not brought into line before it closes, the rule is born
with four unwritten exceptions — which is exactly how the window came to offer
eight kinds of step while the engine runs three.

*In full, with the three properties of an open system and the numbers of the
survey*: `docs/the-four-surfaces.md`.

### Whoever does not declare how they run out does not stand in the middle of a chain

**01/09/2026**, from faults 16, 31 and 32 — which were the same defect seen from
three sides.

An engine that does not declare `ask.unusable_when` **cannot occupy a fallback
position**: it goes at the end of the chain, or it does not go there at all. Not
because its descriptor is wrong — an empty list says «nobody looked», and that
is the truth — but because `says_it_cannot_work` on an empty list is `false`:
its running out passes for any old failure, the step dies on it, and whoever is
behind never starts. A chain `claude-code → agy → codex` had the air of two
fallbacks and had none: `codex`, which declares its own 401, **never started**.

**What makes it red**, without which it would not be binding: the rule lives in
`toolbox::Descriptor::cannot_be_a_fallback`, in one place only, and it is
interrogated by
`every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted` over the
flows of the tree and by `sailor flow check` over the flows of whoever launches
it. It was born red on twelve positions in four flows.

**The thing to remember, which holds beyond this case.** The rule had been
written for a day, and the test that held it was `#[ignore]` with a good reason:
measuring how `agy` says it has run out of quota is impossible until it is seen
doing it, and inventing that word would send a malformed mandate down the whole
chain. But *that was the reason for not inventing a figure, and it had become
the reason for not having a check* — and a rule almost always has **two ways of
being respected**. Measure the words of whoever stands in the middle, or do not
put in the middle whoever does not have them. The second asks for no figure that
does not exist.

**And the check does not only serve to find the defect: it serves to make the
repair safe.** `gemini-cli` declared it could answer a plain question and had no
line to ask it with (fault 32). Writing it was considered dangerous, because an
`ask` without `unusable_when` would have let gemini into the chains with no
fallback — a fourth fault 31 created in order to close 32. As soon as the rule
about position exists, that danger no longer exists, and the line could be
written **measured**: `gemini --prompt` without the question exits 1 and says
«Not enough arguments following: prompt», free and without calling any provider.
Its `usage` and its words for running out stay unmeasured, and therefore
unwritten.

**What this does not concede.** That an `agy` that has run out **at the end** of
a chain says it has run out: it does not say it, it dies with its own error
message. No fallback is lost — there is nobody behind it — and the difference is
read in the reason for the fault, not in the behaviour. The missing measurement
stays written in `agy`'s descriptor, where whoever makes it will find it.

### Looking for the measurement comes before choosing the road that avoids it

**01/09/2026, decided by Theo**, a few hours after the entry above and against
its second half.

The entry above tells the truth and stands: a rule has two ways of being
respected, and taking the second — do not put in the middle whoever does not
declare — asks for no figure that does not exist. But that day the figure
**could be measured**, and nobody had tried. «Do not invent a figure» had become
«do not look for it»: they are two different things, and it is the same shape as
the error that entry tells about, repeated one step further on.

**The rule, now.** Faced with a missing figure, you declare **what was tried**.
The roads, in order of cost: the documentation and the command's help, including
the nested subcommands — `codex exec fork` did not appear in the top-level help,
so the help gets looked at in depth; a real invocation that provokes the
condition without spending; the behaviour with an empty home or with no
credentials. If the measurement comes, it is written **exactly as it came out**.
If it does not come, what is written is **what was tried and what it answered**:
a measured absence is worth more than a supposition, and worth far more than an
absence nobody looked for, which cannot be told apart from laziness.

`agy` was measured this way: `HOME` on an empty directory and the line Sailor
really assembles. It says it cannot work in words of its own, those words are in
the descriptor, and its position in the chain is no longer a consequence of its
silence.

### Where an engine sits in a chain is decided on a measurement, not on a habit

**01/09/2026, decided by Theo.**

Twelve positions in four flows declared the same order and **no document said
why**. An order nobody decided is not a choice: it is a habit, and it defends
itself because nobody knows what would disprove it.

What it gets decided on, and what has to be measured before reordering: **how
much each engine costs** (the price list,
`~/.config/sailor/pricing.json`), **how much quota it has left**
(`sailor remaining`), **whether it is really authenticated** — on the home of
the active profile, not on that of whoever opened the terminal — and **how many
times it has answered**, from the store. An order that does not rest on at least
one of these four is not applied: it gets written in the note `da-fare` as a
proposal.

**And the first outcome of this rule is that two of the four numbers are not
there.** The price list knows one provider out of three, and `sailor remaining`
answers for one engine only: an order by cost or by remaining quota **is not
computable today**, and whoever proposed one would be guessing. The only thing
the measurement imposes today is that an engine measured **not authenticated**
must not stand in front of one measured authenticated. The numbers and the
proposals that remain are in the note `da-fare`.

**The limit of this decision, declared.** Credentials are a state that changes;
the order written in a flow is not. Writing the first inside the second is a
cure that goes stale, and the right shape — that Sailor measure and override at
execution — is a proposal, not a decision.

### A total with an unknown inside it is shown as a floor, never as a figure

**01/09/2026**, from fault 37.

A cost total that contains even **one single** call without `cost_micros` does
not get printed as a number. It is read from `Spend::reading()`, which returns
one of the three cases — nothing, the total, **at least** this — and whoever
shows it writes the sentence that follows: «at least 1.6674, and the true one is
higher: 3 calls out of 4 are not measured». With not even one measurement what
is said is **unknown**, not «at least 0.0000»: the latter is true, says nothing,
and reads as a small spend.

**Why the note beside it was not enough, and it is the part to remember.** The
note was there. `sailor flow cost` already printed «partial: 3 calls with no
known cost», one line under the number, and the handed-over run of the 31/08 A/B
was read as **$1.6674** when it had cost **$7.2080** — 4.3 times. *Whoever reads
a total reads the number.* A qualifier that does not take the place of the
figure qualifies nothing. It holds for every figure Sailor shows, not only for
this one.

**And the rule lives in one place only.** `Spend` told the three cases apart
from the day it was born: the distinction was right in the engine and did not
reach whoever reads, because the only way of interrogating it was a boolean.
Whoever redoes the comparison in their own `format!` creates a second rule that
diverges — and the one to diverge would be the one a person reads, that is, the
only one no type checks.

### A person's quota is not the cost of a run, and the two do not go together

**01/09/2026**, from the second half of fault 37.

Sailor can read how much quota a person has already consumed:
`models::remaining` interrogates Claude Code's OAuth channel — read-only, no
cost — and gets from it
`Remaining { engine, unit, used_fraction, resets_at, observed_at }`. It is the
first thing in the whole system that **measures** a consumption instead of
asking whoever is working for it.

**It does not replace the cost of a step, and the two do not go in the same
box.** That quota counts *all* the sessions of that person: Sailor's run, the
terminal beside it, yesterday's work that falls in the same seven-day window.
Between two instants what can be got from it is how much quota went by, never
how much a run consumed — there is no way of knowing who else was writing in
between. A number taken from there and written beside a step would be a
measurement with the right face and the wrong meaning, that is, the way fault 37
was born, not its cure. This is why it lives in `sailor remaining` and not in
`flow cost`: two numbers in the same report get subtracted from each other.

**Self-declared consumption stays, marked.** `sailor step close --turns` does
not get touched: an agent's declaration is a figure that counts, provided it is
not confused with a reckoning. It stays written with `cost_micros` at `None`, so
that the total containing it reads as a floor instead of a sum.

**It is declared as a capability of the tool, not hard-wired.** The descriptor
of `claude-code` carries `read_remaining_quota`; `codex` carries it `false` —
tried on 01/09/2026 and not managed, with how far it got written in its note,
which is different from impossible. Whoever does not have it goes on working
without knowing how much quota is left, which is the fallback as always. The
permanent constraint «independence from the model».

## Recommended, not yet decided

- **The threshold of a flow that accompanies goes on the price, not on the
  quality.** Measured: the degradation of the quality is not observable (21
  sessions out of 44, a coin toss); the price of continuing grows by 34% and is
  monotonic (37 out of 45). Waiting on a decision of Theo's.

- ~~**The third block has a precondition that has not been done yet.**~~ Done on
  30/08/2026: the front starts together. Two independent six-second steps take
  6.07 instead of 12.07; three take 6.05 instead of 18.14. «Exploiting the
  machine» now has something to rest on.

  ~~**A decision of yours remains**: how many steps per wave.~~ **Settled on
  31/08/2026, and not with a choice: with an arithmetic.** Four is no longer the
  number, it is the ceiling. Under a spending ceiling the width of the front is
  computed by `how_many_fit` from the remainder divided by the dearest call seen
  in that run. The reason it could not stay a constant: **a ceiling is not
  respected with a wide front** — four calls start in the same instant, none of
  them knows about the others, and by the time the first records its own cost
  the other three have already spent. The worst overshoot is not one call's, it
  is that of however many are in flight. With no observed cost at all it does
  not narrow: returning 1 «out of prudence» would make every run with a ceiling
  serial, for ever, on the basis of a number that does not exist.

### You do not add calls to save calls

**05/09/2026**, from the consultation on costs asked of two external engines.

No model-based router choosing the sitting, no summariser between one step and
the next, no compression of the context entrusted to an engine, no automatic
judge on every step. They are the four shapes a system like this adopts in order
to look intelligent, and each of them adds at least one turn.

**The measurement that decides it.** A four-step flow costs **2.07 times** the
turns of a single session doing the same work, and reads only **8%** more per
turn. The reckoning is decided by the turns, not by the weight of each of them:
compressing the context touches the 8%, adding a service step touches the
multiplier. An optimisation that pays for one call to shorten another comes out
at a loss, and it also hides how the steps pass information to each other, which
is the permanent constraint of clarity for whoever is looking.

**The two engines agree, interrogated separately.** The second added the figure
the other way round: bringing back to a single session the work that today sits
in four steps takes away **52%** of the turns (1 − 1/2.07). It is a reduction of
turns, not a saving measured on the bill, and it is to be read that way.

**What would overturn it.** A measurement showing a summarising turn reduce the
following turns by more than it adds. Until that measurement exists, a proposal
of this family is discarded without discussing it.

**The other side of the decision, which does get done.** If a step is not added
to judge, what closes a run has to be a **deterministic check**: where the
acceptance criterion is executable, the code runs it, and an engine is called
again only on what stays unresolved. A model step that approves is exactly the
extra call this decision forbids.

Where the saving really is: in the steps that carry on the same session instead
of opening a cold one — on one measured call the cache write was **96%** of the
cost — and in not redoing at relaunch the steps that already succeeded.
