# The four surfaces: what Sailor exposes, and what gets composed instead

**31/08/2026.** Born from a question of Theo's — «which product nodes is the
system missing?» — and from the wrong answer it got first: a list of holes. A
list of holes ages in a week and does not say where to put the next thing. This
document tries to say the same thing in a way that holds: not *which nodes are
missing*, but **which surface the system exposes**, so that the question becomes
«which power do we not have yet, and which flow proves it to me».

## Where it comes from

On 31/08 a body of shell scripts from elsewhere — a development-environment
orchestrator, whose survey is not published here — was read with one question:
«which of these become Sailor flows». The survey answered something else, and
that answer is the whole of this document: the entries that could not migrate
were not asking for one node each. They were asking for **five powers** Sailor
did not have — keeping a process alive, killing one, speaking over the network,
carrying a secret without writing it down, returning a value instead of an
outcome.

At the same moment, seven building sites open on Sailor were building
`supervisor`, `terminal`, `presence`, `mcp` — that is, those powers, each in its
own shape, with no shared criterion for what they were.

The defect this document means to prevent already has a measured precedent:
**the window offers eight kinds of step and the engine runs three**, and nobody
had noticed, because the kinds do not live in one single place.

## The boundary, which is already written down

The permanent constraint says: *«we write in code only what touches the world…
the boundary is the power, not "runs versus decides"»*. Two sentences follow
from it, and they are the whole document:

- **The code exposes powers.** Not nodes: powers.
- **Flows compose powers.** An orchestration is a data file that lines up powers
  the engine already exposes. **If an orchestration calls for new code, a power
  is missing — not a flow.**

## The four surfaces

Every registered action belongs to one of these alone, and declares it.

### 1. `sense` — reading the world without touching it

Live processes, ports, load, disk, network, the state of a repository, the index
of the code, and the spend: how much has gone, how much is left, on which
provider.

Two properties make an action a sensor, and the second is the one that gets
forgotten:

1. it changes nothing;
2. **it tells «zero» apart from «I cannot see»**.

The second comes from fault 12: inside the perimeter `pgrep` answered empty
*without an error*, and a watch declared «no flow running» while two were
running. A blind sensor that answers zero is worse than an absent one, because
the flow downstream trusts it.

### 2. `act` — touching the world

Starting and stopping a process, writing a file, calling an engine, making a
network request, committing.

An actuator declares **what it can break**: it is the Bazel model already
decided on 29/08 — a step declares what it needs, and the rest does not exist
for it. An actuator that does not declare it does not get registered.

### 3. `remember` — the store, as a source you put questions to

Runs, costs, faults, decisions. Not an archive: a thing you interrogate. The
distance still to close is written in a mandate from August — *«Sailor records
everything that happens and never goes back to read it»*.

### 4. `gate` — who may do what, and where a person comes in

Human permission is not a special node: it is the declaration that certain
powers, in certain contexts, want a signature. It follows from two permanent
constraints already written down — «whoever creates does not judge», and the
human judgement that stays above the cycle. As long as it lives outside the
system, every gate is a custom and not a mechanism.

## What an action has to declare, to be registered

1. **the surface** — one alone out of `sense`, `act`, `remember`, `gate`;
2. **the powers it demands** — network, disk, processes, money, secrets;
3. **what it answers when it cannot answer** — required for `sense`, and it is
   fault 12 made impossible.

The names of the surfaces are in English because the compiler reads them;
everything a person reads stays in Italian, as already decided on 31/08.

## What «open» means, in three properties

**You ask, you do not know.** A flow does not hold the list of what exists: it
interrogates it. It is the cure already written for tools — *«ask the engine, do
not keep a list»* — carried over to powers, skills and providers. Once it holds
everywhere, the question «which nodes are missing» is no longer needed: the
system answers it.

**It is added without recompiling whoever uses it.** A new power, a third
party's included, comes in with a descriptor — the same shape by which an engine
comes in. It is already product direction number 3: *«an external project plugs
in as a new action; there is no mechanism to invent, there is a project to
choose»*.

**It travels.** A flow declares everything it demands, and whoever receives it
either runs it or **is told why it cannot**. Today it is the other way round:
fault 17 (skills present on one machine only, undeclared) and fault 25 (the
repository root written inside the flow) are the same defect seen from two
sides.

## Why writing it down here is not enough

*«Whoever writes a new rule writes what makes it red as well.»* This one has its
own check, and without it it does not come in:

> a test that walks the action registry and **fails if an action does not
> declare its own surface and its own powers**; and, for `sense` alone, fails if
> it does not declare what it answers when blind.

**That test was never written, and the choice was made on 04/09/2026: this
document stays declared and not in force.** No action carries a surface —
searching the crates for `surface` returns two files, and one of the two,
`crates/actions/src/history.rs`, says so of itself. It is not withdrawn, because
the design holds; it is not built now, because it only pays off together with
the model of a step's powers, which comes later. Until then, by this very
project's own rule, **what is here is an intention and not a constraint** — and
it is fault 67, which stays open and now has an answer instead of a question.

It is born red on today's nine actions — none of them declares anything — and
that is how this page stays alive instead of turning into the description of
what has already happened.

## The debt this document declares

The seven building sites open on 31/08 produced new crates **before** this
criterion. Bringing them into line is a decision of Theo's: if they are not
brought into line before it closes, the rule is born with four unwritten
exceptions, which is how the window got to eight kinds against three.
