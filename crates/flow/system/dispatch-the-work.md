# `dispatch-the-work` — the flow that splits one mandate between three engines

A **trigger** receives the mandate and makes it available to the graph. One node
splits it into two assignments that stand on their own. Two engines carry them
out **without seeing each other**. A third model reads the assignments and the
two answers and gives a judgement. A last node turns that judgement into an
outcome: it passes, or the flow is red.

## The six nodes

| node | tool asked for | depends on | what it does |
|---|---|---|---|
| `trigger` | — | — | waits for a signal and makes available its text, who sent it and from where |
| `dispatch` | `claude-code` | `trigger` | splits the mandate into two assignments: `first_engine`, `second_engine` |
| `engine_a` | `codex` | `dispatch` | carries out the first assignment, read-only |
| `engine_b` | `agy` | `dispatch` | carries out the second assignment, read-only |
| `verify` | `claude-code` | the three above | reads everything and writes `verdict` and `why` |
| `verdict` | — (`shell_check`) | `verify` | accepts **only** the approval: this is where the flow turns red or green |

`engine_a` and `engine_b` do not see each other: each one receives only
its own assignment, and has never seen the whole mandate nor the other's
answer.

## The four rules this file keeps

**1. In the flow lives *how* the work is dispatched, not *what*.** No step
carries an assignment written inside it. The work all enters through the
trigger: to give it another one you change the text in `inputs.trigger.text` —
or you press the button in the window, which writes the same field — and the
graph is not touched.

**2. No step names a binary.** Every step that runs an engine declares
`"tool": "<identifier>"`, the same one the tool detector
(`crates/toolbox`) returns, and whoever runs it resolves it on the machine of
whoever launches. On a machine where that tool is not there, the step stops
**before spending anything** and says which one was missing and where it looked.
Before 28/08/2026 it said `"bin": "claude"`, and that flow ran only
where that name was on the path of whoever ran it.

**3. No step is special.** The verifier is not a node of a kind of its own: it
is a step like the others, which receives a piece of work to verify. Whoever
runs it is changed with one line (`tool`), and the role is written in the
prompt.

**4. Every step declares the shape of its own answer.** In `answer_shape`.
That shape ends up in the engine's prompt — with a reference `{"$json":
"/answer_shape"}`, written once only, so the two copies cannot diverge — and it
is enforced on the answer. To the next step passes **only** what the shape
declares: preambles, reasoning and greetings do not cross the chain and are not
paid for at every call downstream.

## When this flow turns red

A step breaks, and whoever depends on it does not start, if the engine:

- exits with a code other than zero (`engine_exit_error`);
- does not answer within the time ceiling (`engine_timed_out`);
- does not start (`engine_spawn_failed`);
- is not there on this machine (`tool_unavailable`);
- answers something that is not JSON (`answer_not_json`) or that does not keep
  the declared shape (`answer_off_shape`).

And at the end, if the verifier rejects, the `verdict` node closes red.

A step may declare that an outcome is acceptable — `"accept": ["exit_error"]`
— for whoever runs a check made on purpose to see it fail. No step of this
flow does so, and if it did it would have to say so in its own output schema
too, where it shows by reading the graph.

## How it is launched

From the root of Sailor's sources, wherever that is on your machine:

```bash
cargo run -p sailor -- flow run dispatch-the-work
```

The folder counts: the command looks for flows in `flows/` under the current
one. To look at it without running it: `cargo run -p sailor -- flow check dispatch-the-work`.

## Something this flow seemed to say that is not true

Until 28/08/2026 this document and the flow's description said that the two
engines run «insieme». **That is not so, and it is measured**: the executor
walks the front of the ready steps **in order, one after the other**. Two
six-second steps take twelve, not six.

The code does not hide it — `crates/flow/src/executor.rs` declares it in the
exact place: «questo esecutore lo percorre in ordine: l'esecutore di processi
potrà avviarlo in parallelo». It was the flow and this document that said
something else.

It remains true that the two engines **do not see each other**: neither of them
receives the other's answer, and this is what makes `verify`'s verdict a
judgement on two independent pieces of work. It is «insieme» that described a
parallelism that is not there, and the cost is time: two artificial
intelligence engines in single file make you wait for the sum, not the maximum.

## The first real run, 28/08/2026 — and what it taught

Outcome: **red**, and for the right reason. The chain ran all the way through,
so the engine, the dispatching and the passing of values between the steps
**worked**. What fell was the content: the two engines had exited in error with
empty output, and the verifier had written that there was nothing to verify.

But the real fault was another one, and it was worth more than the two misuses:
**the two failed steps had been recorded as having gone well**, with
`status: exit_error` inside the result. The flow had turned red only
because *the last* node also looked at the state of the engines — a net
somebody could have taken away without noticing.

Now it is no longer so, and the final node no longer looks at anybody else's
states: it cannot even see them, because a broken step never reaches it.

### What was measured afterwards, command line by command line

- **`agy`**: the prompt goes in an **argument**, not on the input, and `--mode`
  goes **before** `--print`. `agy --mode plan --print '<prompt>'` answers and
  exits 0 (tried on 28/08/2026). The old form was `--print --mode plan`, where
  `--print` took `--mode` as its own prompt.
- **`codex exec`**: it reads the prompt from stdin when it is not an argument
  (`codex exec --help`, measured). The failure of the first run is still not
  explained: to be tried again by hand before the next run.
- **`claude`**: unchanged, `-p --model <name>` with the prompt on the input.

## The trigger: what is true and what is not

The `trigger` node is the flow's real entrance, and the signal sources are
a **list of descriptors** (`crates/trigger/descriptors/default.json`), not
code: they are added by writing a line of JSON in `~/.config/sailor/triggers.d/`.

- **`manual`** — true and working. Somebody presses and it starts, carrying a
  text. It is the source the window's launch button will use.
- **`sailor-terminal`**, **`orca-terminal`** — declared and **not listened to**.
  A step that uses them breaks with a message that says what is missing. There
  is no simulated listening: a fake signal would start the engines downstream,
  and that costs real calls.

Why listening to a terminal cannot be done honestly today:

1. **No Sailor process stays up.** `sailor flow run` runs the graph once and
   finishes: there is nobody waiting for a signal and starting a run when one
   arrives. This is missing **before** any reader.
2. **Sailor's terminal does not exist yet**: nobody writes the file the
   descriptor declares. The path written there is the shape it will have, not a
   measurement.
3. **Orca's panel register is not an honest source** (measured on 28/08/2026):
   `terminal-history/*/output.log` is a binary frame format with ANSI terminal
   bytes inside — screen redraws, not messages — emptied in place beyond 5 MB
   and written in batches every ~5 seconds. A reader queuing behind it loses
   content without noticing, and rebuilding the text would mean rewriting a
   terminal emulator. The only supported way is
   `orca terminal read --json --cursor N`, which returns text: it is the shape
   declared in the descriptor, and it needs a reader that keeps the cursor from
   one run to the next.

## The perimeter, declared and not enforced

The two engines are invoked read-only — `--sandbox read-only` for the first,
`--mode plan` for the second, no option that widens the permissions of the two
steps that use Claude. It is a declaration in the arguments, **not a limit
anybody enforces**: the field that ought to say where a flow may write exists,
but nobody reads it. Open entry:
`2026-08-28-il-perimetro-di-un-flusso-non-limita-niente.md`.
