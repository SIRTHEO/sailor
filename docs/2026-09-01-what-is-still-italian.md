# What is still Italian, and which half of it is dangerous

Measured on 01/09/2026, on the trunk, by eight agents reading the code — four
censusing and four judging what they had not written. Written in English because
this is a document about the conversion, and the conversion's own rule is that
English is what ships.

## The number, and why the obvious way of getting it was wrong

**320 Italian string literals in the crates.** A grep found 123. The difference
is not carelessness: a regular expression matching Italian function words scores
`"history-per-flow"` as Italian on `per`, misses every literal that spans lines,
and cannot tell prose from a value. It was wrong in both directions at once.

| what it is | how many |
|---|---|
| **for a person to read** — command-line output, error text, help | 228 |
| **a value the code compares or stores** | 22 |
| internal — `panic!`, `expect`, fixtures | 21 |
| not Italian at all (false positives) | 49 |

The judges moved five entries. **Four went towards `machine value`** — three from
`for a person` and one from `not Italian`. That is the direction that costs:
translating a sentence is harmless, and leaving a value in Italian keeps Italian
inside the data format, where whoever compares it has no idea.

## The dangerous half: 26 values, not sentences

A value is compared with `==`, matched, serialised, or typed by a user. Renaming
one is a contract change, so each needs its own decision — and each needs the
change made **in the same edit as everything that reads it**.

### Already done, 01/09

The origin family of a flow, which `sailor flow list` prints on every row and the
window shows beside every source: `di sistema`, `tuoi`, `dichiarati`, `del
progetto`, `del progetto (nessun sailor.json…)` and `(spediti col prodotto)` are
now `built in`, `yours`, `declared`, `this project`, `this project (no
sailor.json: root guessed)` and `(shipped with the product)`. Both constants
carried a comment asking for exactly that: move with the assertions, in one edit.
Two of the six had no constant at all and were written out where they were used
— a family that lives as scattered literals is a family only in the comment.

`crates/terminal/src/routing.rs` had two more: `BUILTIN_SOURCE = "incorporato"`,
which travels in the `source` field of `Loaded` and `Problem`, and `format!("la
voce numero {}", index + 1)`, which is the **fallback for a missing `id`** — when
a catalogue entry declares none, that phrase *becomes* the identifier, and a test
compares it. A fallback standing in for an identifier is an identifier.

**Neither needed a decision, and the census is why that was not obvious.** The
sibling crates had already settled both: `trigger` and `models` say `built-in`,
`trigger` and `toolbox` say `entry number {}`. Terminal was the only one behind,
with its neighbours already agreeing. **This census asked «what is Italian?»,
which cannot answer «it is already written next door»** — and the second question
is the one that turns a design decision into copying a line. Ask it first: for
every value, does a sibling crate already spell this?

The same reading argued **against** a repair. `entry number 3` points at a count
rather than at an entry and moves if the file is reordered — but it does so
identically in all three crates, so fixing one turns three coherent things into
two coherent ones and a surprise. Either all three or none.

### The rest, with what makes each one hard

- **Words a person types.** `sailor flow cap <name> nessuno`, `sailor flow
  schedule <name> … leggero|pesante`, and the `--kind competenza|agente|comando|
  regola|gancio` aliases. These are the command line's public vocabulary:
  changing them breaks whoever wrote them into a script. The way through is to
  accept the English word and keep the Italian one as an alias — and note that
  the `--kind` help already lists only the English forms, so those five aliases
  are undocumented today.
- ~~**Names of the shipped flows.**~~ Done on 02/09 and **without aliases**, by
  decision: `what-this-machine-has`, `migrate-to-sailor`, `dispatch-the-work`.
  Each name was three things at once — a registry key, an argument a person
  types, and a string inside `crates/terminal/descriptors/default.json` — so it
  moved in all three at once or the routing rules would have pointed at nothing.
  Whoever named one of them in a flow of their own gets «no flow by that name»,
  which is the cost that was weighed and accepted.
- **`crates/supervisor/src/main.rs`.** `"ignoto"` is the fallback for `$USER`
  inside `sailor-live/{who}/{pid}` — a machine-visible name.
- **`crates/faults/src/lib.rs`.** The worst of them, and the language is the
  smaller half. `Fault::still_open()` is
  `status.starts_with("**aperto**") || status.contains("chiuso in parte")`, so a
  status it does not recognise **counts as closed**. «Not open» and «I do not
  know this one» give the same answer, and the total moves in the reassuring
  direction. Translating that column one row at a time would drop faults out of
  the count in silence. The cure is not an enum instead of prose — the prose says
  which half of a cure is done, and that is worth keeping — it is a **third
  answer**, `unrecognised`, that something refuses. And there are two
  implementations of the predicate: the crate reads the strings,
  `tests/the_fault_table_holds_together.rs` reads two extracted booleans.

## The other half: 228 sentences

Not dangerous, just unfinished. Where they are:

| file | sentences |
|---|---|
| `crates/sailor/src/flow_cmd.rs` | 100 |
| `crates/supervisor/src/main.rs` | 29 |
| `crates/sailor/src/step_cmd.rs` | 17 |
| `crates/terminal/src/routing.rs` | 12 |
| `crates/sailor/src/session_cmd.rs` | 12 |
| `crates/faults/src/lib.rs` | 11 |
| `crates/sailor/src/inventory_cmd.rs` | 9 |
| everything else | 38 |

The census proposed a key for each, under `cli.flow` (101), `cli.step` (16),
`cli.inventory` (14), `terminal.route` (13), `cli.session` (12), `fault.column`
(11) and `run.said` (8).

**Two things a reader of that list should know.** Several literals are *mixed* —
an Italian label around an already-English clause, or an Italian auxiliary
(`ha` / `hanno`) chosen in Rust and dropped into a sentence that is otherwise
English. And singular/plural is picked in Rust before the sentence is built, so
each of those needs two keys: the catalogue has no plural rules and should not
grow any.

## What the catalogue covers now

`i18n/en.json` and `i18n/it.json`, read by the window through
`desktop/src/i18n.ts` and by everything else through the `catalogue` crate, which
embeds them with `include_str!`.

**53 failure classes out of 53.** This morning it was 15 of 52: the other 37
reached the window and fell on `tryT(...) ?? failure`, so whoever hit one read
`subflow_too_deep` where a sentence belonged, and nothing went red. Five of them
could not even be searched for, because `subflow.rs` built the name with
`format!("subflow_{status}")`.

A test now pairs the two lists in both directions and has no seed: a class
arriving without a sentence is a defect, not a backlog.
