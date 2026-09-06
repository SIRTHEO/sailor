# A single delivery — Sailor's window

This file is **a prompt**, not a report. It is handed whole to an engine that
has never seen this tree, and it stands on its own: inside are the charter, the
shape, the measure and the way to close. Whoever runs it must not have to ask
questions.

It lives here and not in a flow because a prompt that lives in one place only
is corrected in one place only. `porta-una-superficie-alla-carta` reads it from
the `design-charter` deposit; a person reads it from here; they are the same text.

---

## 1. What you deliver

One pass over the window in `desktop/`, which lifts the surface by one notch
and leaves the tree green. Not a rebuild: **one entry per run**. At the end:

- the tree passes `npm test` in `desktop/` and `cargo test` at the root;
- `npm run screenshots` and `npm run check:canvas` run and their outputs are up
  to date in `desktop/design/screenshots/`;
- one commit per piece that stands on its own, with the **why** written in it;
- the verdict at the bottom of this file, filled in.

If one part is blocked, everything else gets done and what was left out is
said. Narrowing the scope on your own is not a decision for whoever runs this.

## 2. What this product is

Sailor watches machines that work. A **flow** is a graph of steps; a **step**
ends in one of six ways; a **run** is a flow that has been executed, with what
it cost. The window stays open all day beside a terminal.

Everything else follows from this: *beautiful* here means **readable**. The
thing that must shine on the screen is the work in progress, not the frame.

## 3. The charter

The ground is night. `:root` is the dark scheme; `@media (prefers-color-scheme:
light)` brings the daytime twin. There is no third palette: every role a
library component asks for is an **alias** of a role of the sheet.

The twelve prohibitions are written at the head of `desktop/src/styles.css` and
are not summarised here: they are read there, where they hold. The ones most
often got wrong:

1. **Three type roles, two faces.** No `system-ui` by default.
2. **Two radii and one pill.** The pill belongs to the state badges, to nothing else.
3. **One shadow only**, and only for what really floats.
4. **Colour is reserved for the machine's state**, plus *one* accent that
   means «the action». A second accent is a second meaning.
5. Colour never carries a state on its own: the word stands beside it.
6. **No text/ground pair below 4.5:1.** Measured, not estimated.
7. **Two levels of text, not three.** Emphasis is weight, small caps, spacing.
9. No gradients, frosted glass, blurs.
11. **A rigid column declares how it behaves when narrow.**
12. The ground is never pure black.

## 4. The shape

One single column on the left, like Orca. The column **is the world**; the main
area shows one thing at a time, at full width.

```
⌘K  search or launch a command
──────────────────────────────
▮ Terminals   ▤ Ledger   ◷ Runs   ⚓ Sailor      ← what holds everywhere
──────────────────────────────
WORKSPACES
▾ a-project
   ▾ a-tree  ●                                   ← the tree you are standing in
        ◈ Board                    31
        ⌁ a-flow                 7 steps
        + New flow
     another-tree                                ← same project, another branch
──────────────────────────────
FLOWS EVERYWHERE
   yours    ⌁ …
   built in ⌁ …                                  ← the system flows
──────────────────────────────
OUTSIDE EVERY WORKSPACE
   ▮ a-terminal                                  ← outside is a place, not an absence
```

Four requirements, and they are the client's:

1. **The workspaces are there**, and the flows of a workspace sit with its board.
   A workspace has **several trees** (one checkout per branch): the name groups,
   the path is the identity.
2. **The system flows have a place.** Buried among those of a checkout they
   read as belonging to that checkout.
3. **There is a view of all the linked global flows**: nodes = flows,
   edges = `subflow` calls. This is the only thing on the list that does not
   exist yet.
4. **The terminals are there, as in Orca**, and a terminal **can sit inside a
   workspace or outside**. Outside is a place with a heading of its own.

## 5. What «strong» means

The floor is measured by the tests: a typeface outside the roles, a pair below
threshold, a class no rule dresses. **Nobody measures the ceiling**, and an
engine working against a battery produces the minimum that passes — correct and
flat.

So you look at the screen and you judge on **four separate axes**, never on one
alone (on one axis alone the judgement agrees with the human eye 48.5% of the
time; on four, 69.5%):

| axis | the question |
|---|---|
| `visual_task` | does the scene show what it exists for? |
| `aesthetic` | does it look like somebody decided it? |
| `code_task` | does the code do what the scene promises? |
| `code_quality` | and does it do it in a way that reads back? |

Every scene also carries `lowest_because`: **one sentence on what kept it low**.
A score without that sentence is not a judgement, it is a number.

### The signs of the minimum that passes

If you find one, it is work, not taste:

- a hierarchy that is not there: three weights that count the same;
- vertical rhythm at random — spaces that do not sit on a grid;
- the same name written twice in the same bar;
- a **destructive action given prominence** where an ordinary action belongs;
- a border around everything: the box used in place of spacing;
- a column that truncates instead of giving its width to what needs it;
- the empty, the loading and the fault not drawn: they are the states nobody
  looks at, and that is where the default survives longest;
- a component library installed and never used.

## 6. The cycle

Five passes, and **you keep the best, not the last**. Rewriting without looking
at the screen again is worth +1.5%; **looking at it again on every pass is
worth +17.8%**, and half of that gain lies in holding on to the best pass,
because the trajectory does not always rise.

On every pass, in this order:

1. **redraw** — `npm run screenshots`;
2. **look** — first the tree (`*.aria.txt`, a few tens of kB for all the
   scenes), then only the images of the scenes the tree does not explain;
3. **mark the four axes** and the sentence of the lowest;
4. **change one single thing** and go back to point 1.

While the look improves by 26.3% the quality of the code falls by 3.2%: this is
why **the tests always close**, and they run **twice** — before anything is
touched, to know what was already red, and afterwards, where nothing is
tolerated.

## 7. The measures, and who takes them

| command | where | what it answers |
|---|---|---|
| `npm test` | `desktop/` | the floor of the window |
| `npm run screenshots` | `desktop/` | twelve scenes, image + tree |
| `npm run check:canvas` | `desktop/` | does the canvas exist at every width? |
| `cargo test` | root | everything else, ratchets included |

`npm run screenshots` exits 1 only if a scene was missed **without** the
product declaring the gap. A declared gap (`product gap: …`) is a fact put on
record, not a failure of the capture: it is left there, and it is named.

## 8. The process prohibitions

- **The gate before the repair.** If a pass calls for a check, it is written
  *before* repairing the fault it must catch, and **it is watched failing**. A
  gate written afterwards is born green and has never shown it can see
  anything.
- **A red that was already there is not yours.** It does not get repaired: it
  is not the work chosen. But it does get said that it was there.
- **A ratchet only goes down.** If a measure has fallen, it is rewritten with
  the measured number; it is never raised to let something through.
- **A check is not deleted because it is a nuisance: it is adapted.** If a gate
  goes red after a change, the first hypothesis is that it is right.
- **No literal colour in a component**: a hexadecimal inside a `.tsx` answers
  to no scheme.
- **One commit per piece that stands on its own**, and the message says the
  why, not the what. The what is in the diff.

## 9. How it closes

```json
{
  "passes": 5,
  "scores_per_pass": [
    {"pass": 1, "visual_task": 0, "aesthetic": 0, "overall": 0, "what_changed": ""}
  ],
  "kept": 4,
  "why_kept": "",
  "redrew_every_pass": true,
  "touched": ["path/to/a/file"],
  "gate_written": "the name of the test, and what it saw fail",
  "reds_already_there": [],
  "left_out": []
}
```
