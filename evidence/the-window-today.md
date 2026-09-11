# The window as it stands

Captured at `3b58c759` with `npm run screenshots`: six scenes at 375 px and
1440 px, twelve captures, `missing.txt` says none was missed. I read every
accessibility tree, and the images of the scenes the trees did not explain
(`flows-canvas-1440`, `flow-in-focus-1440`, `step-selected-1440`,
`step-selected-375`, `now-1440`). Where a score rests on the tree alone the
sentence says so.

## The fact that shadows every score

**Half the battery photographs a product that is not running.** The capture runs
in a browser with no Tauri shell behind it, so `now`, `installed` and `history` —
six of the twelve captures — contain no data at all. What is photographed is the
refusal sentence:

- *I cannot tell you what waits: outside the desktop shell there is no engine to ask*
- *I cannot sweep this machine: outside the desktop shell there is no machine to sweep*
- *Cannot read the history: outside the shell: the engine reads the history*

The scene comment for `now` calls this a feature — "this doubles as the failure
scene". It is one scene doing two jobs and doing the second badly: nobody has
ever looked at a picture of the opening view with work in it. **The three data
views of this window have never been photographed.** So has the terminal: there
is no terminal scene at all, and the terminal is what Theo wants to move into.

## The scores

0–10 on four axes. `visual_task`: does the scene show what it exists for.
`aesthetic`: does it look like somebody decided it. `code_task`: does the code do
what the scene promises. `code_quality`: does it read back.

| scene | width | visual_task | aesthetic | code_task | code_quality |
| --- | --- | --- | --- | --- | --- |
| now | 375 | 2 | 2 | 7 | 7 |
| now | 1440 | 2 | 2 | 7 | 7 |
| flows-canvas | 375 | 5 | 4 | 7 | 6 |
| flows-canvas | 1440 | 6 | 5 | 7 | 6 |
| flow-in-focus | 375 | 5 | 5 | 8 | 6 |
| flow-in-focus | 1440 | 8 | 7 | 8 | 6 |
| step-selected | 375 | 2 | 3 | 4 | 6 |
| step-selected | 1440 | 4 | 5 | 6 | 6 |
| installed | 375 | 1 | 2 | 7 | 7 |
| installed | 1440 | 1 | 2 | 7 | 7 |
| history | 375 | 2 | 4 | 7 | 7 |
| history | 1440 | 2 | 4 | 7 | 7 |

Median `visual_task` **2.5**. Median `aesthetic` **4**. The window's code is in
better shape than the window.

## `lowest_because`, one sentence each

**now-375, now-1440** — The failure state is one unstyled muted paragraph flush
against the top-left corner of eleven hundred pixels of empty ground, with no
container, no icon, no vertical centring and nothing to do next, so the view the
product opens on is a void and an apology.

**flows-canvas-375** — Read from the tree: the rail and the head stack above a
canvas holding one step, and the same 85 % emptiness as at 1440 arrives in a
window eight times narrower.

**flows-canvas-1440** — With a single step to show, the two nodes sit in the
top-left eighth of the canvas and the other seven eighths are empty grid: the
view never fits what it holds, so the eye has to hunt for the only thing on it.

**flow-in-focus-375** — Read from the tree: the same seven-step lane that reads
at a glance at 1440 has to survive a 375 px canvas, and nothing in the capture
says it re-flows rather than scrolls.

**flow-in-focus-1440** — This is the best thing in the window and its lowest axis
is still `code_quality`: the lane, the states and the dashed cord to the running
step all read, but the four zoom controls under it render as four blank squares
with no glyph inside them.

**step-selected-375** — The inspector takes about 255 of 375 CSS px and leaves
the canvas roughly 87, which slices the add-step buttons in half (`TRIGG`,
`AGEN`, `CHEC`) and truncates the flow description mid-word.

**step-selected-1440** — Opening the panel cuts the last two of the seven steps
off the right edge because the canvas does not re-fit when the width beside it
changes, so the densest case is the one case where the flow stops being legible.

**installed-375, installed-1440** — The scene exists to show what this machine
has installed and it shows two refusal sentences in identical muted prose, one
under the other, on an otherwise empty screen.

**history-375, history-1440** — The four tabs under `Why` are the one piece of
real hierarchy in the capture, each with a label and a hint, and underneath them
sit two refusals where every run this product has ever recorded should be.

## What the scores are not

No score here is about the code being wrong. `code_task` sits at 7 in most rows
precisely because the window refuses honestly instead of inventing a plausible
machine — that is a decision this tree took on purpose and it holds. The gap is
between 7 and 2: **the code is careful and the screen is undesigned.**
