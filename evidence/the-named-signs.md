# The named signs, hunted for one by one

The work queue for the window. Every line names the scene, the element and what
it should be instead. Nothing here is taste: each is one of the signs the
assignment named, found or not found in the twelve captures at `3b58c759`.

## 1. A hierarchy that is not there — three weights that count the same

**FOUND, and it is the worst one.**

- **Scene:** every capture. **Element:** the left rail.
  The four places (`Waiting for you`, `the work`, `Why`, `the machine`), the
  `sample data` note, the flow rows (`relay`, `prima-corsa`), the workspace row
  (`Board`) and `+ New flow` are **all drawn as the same 1 px-bordered rounded
  box at the same weight**. Navigation, content and a creation action are
  typographically indistinguishable; only a left dot separates a flow from a
  place. **Should be:** the four places carry the rail's only strong weight and
  no border at all; the flow rows lose their box and become rows separated by
  space; `+ New flow` is the one bordered thing in the column.
- **Scene:** `step-selected-1440`. **Element:** the inspector's section labels.
  `RELAY`, `SELECTED STEP`, `ACTION`, `SPECIES`, `MAX ATTEMPTS`, `RUNS WHEN`,
  `INPUT AND OUTPUT`, `DEPENDS ON` are **eight labels in one identical uppercase
  tracked treatment**, so the flow's name, the panel's purpose and six field
  captions all read as peers. **Should be:** the flow name and `Chain brake` are
  the panel's heading; the six field captions drop to sentence case at
  `--text-small` in `--text-2`.

## 2. Vertical rhythm at random — spaces off any grid

**NOT FOUND in the sheet. FOUND in one render.**

The grid is declared and used: `--space-1` through `--space-7` are 4, 8, 12, 16,
24, 32, 48 px, and `stylesheet.test.ts` has a test named *"no size written by
hand"*. I found no space off the grid in the stylesheet.

- **Scene:** every capture. **Element:** the rail, between `+ New flow` and the
  broken-flow error above it. The four red lines of
  ``unknown field `retries` at line 14`` sit directly in the column with no box,
  no left rule and no gap of their own, so the rhythm that holds for eleven rows
  breaks at the twelfth. **Should be:** the broken flow's reason sits inside the
  flow row it belongs to, indented under `notte`, at `--space-2`.

## 3. The same name written twice in the same bar

**FOUND, four times over in one screen.**

- **Scene:** `step-selected-1440`, `flow-in-focus-1440`. **Element:** the top of
  the window. `relay` is written **four times above the fold**: the breadcrumb
  `Board › relay`, the flow chip `relay 7 steps`, the lane header
  `relay 7 STEPS`, and the panel's `RELAY`. **Should be:** the breadcrumb keeps
  it; the chip drops the name and keeps `7 steps` and the description; the panel
  drops `RELAY` entirely, since the panel is inside the flow the breadcrumb names.
- **Scene:** `now-1440`. **Element:** the banner. `Waiting for you` is the
  breadcrumb **and** the highlighted first rail row, an inch apart.
  **Should be:** the breadcrumb shows the place only when it is deeper than the
  rail selection.
- **Scene:** `flows-canvas-1440`. **Element:** `prima-corsa` appears three times
  — breadcrumb, chip, lane header — and the description *"The smallest flow
  there is: one check alone."* appears twice, in the chip row and again inside
  the lane header.

## 4. A destructive action given prominence where an ordinary action belongs

**NOT FOUND in the drawing. FOUND in the keyboard, which is worse.**

Deleting a flow is correct: it is a `DropdownMenuItem variant="destructive"`
behind the `⋯` menu (`App.tsx:1905`) and it asks *"There is no way back from
here."* (`App.tsx:1137`). `Delete step` sits at the very bottom of the inspector.
Neither is prominent.

- **Scene:** `flow-in-focus-1440`, `step-selected-1440`. **Element:** the canvas.
  `deleteKeyCode={["Backspace", "Delete"]}` (`App.tsx:1607`) routes into
  `onNodesDelete` (`App.tsx:1194`), which calls `deleteStep` **with no
  confirmation and no undo**. A selected step plus the most ordinary corrective
  key on the keyboard removes it. Deleting a whole flow asks; deleting a step by
  Backspace does not. **Should be:** either the same confirmation the flow gets,
  or an undo in the bar for the length of the session.

## 5. A border around everything — the box in place of spacing

**FOUND, and it is the signature of this window.**

- **Scene:** every capture. **Element:** the rail. Eleven consecutive bordered
  boxes, described in sign 1.
- **Scene:** `step-selected-1440`. **Element:** the inspector. `ACTION`,
  `SPECIES`, the species sentence, `MAX ATTEMPTS`, `RUNS WHEN` and
  `INPUT AND OUTPUT` are each inside their own 1 px box, six boxes stacked in one
  column, **and three of them hold text that is not editable**. A box around a
  read-only sentence reads as a disabled input. **Should be:** only the two real
  inputs (`Max attempts`, the action combobox) keep a border; the rest is
  separated by `--space-4`.
- **Scene:** `flows-canvas-1440`. **Element:** the `sample data` note, drawn as a
  dashed-border box in the rail. **Should be:** a line of `--text-3` under the
  section heading, no border.

## 6. A column that truncates instead of giving its width to what needs it

**FOUND twice, and the check that exists for it cannot see either case.**

- **Scene:** `step-selected-1440`. **Element:** the canvas beside the inspector.
  Opening the panel cuts `signal-is-gone` and `send-the-start` — the last two of
  the seven steps — off the right edge. `flow-in-focus-1440`, the identical flow
  with the panel closed, shows all seven comfortably. **The canvas never re-fits
  when the width beside it changes.** **Should be:** `fitView` on the width
  change, or the panel overlays rather than displaces.
- **Scene:** `step-selected-375`. **Element:** the same, at the narrow width. The
  inspector holds roughly 255 of 375 CSS px and the canvas gets about 87. The
  add-step toolbar is sliced mid-word — `TRIGG`, `AGEN`, `CHEC` — and the flow
  description truncates as `Resets a full session and h`. **Should be:** below
  the narrow threshold the inspector is a sheet over the canvas, not a column
  beside it.
- **Why no check caught it:** `npm run check:canvas` reports
  `rail null · canvas 375 · panel null` at all four widths. The script never
  selects a step, so `.panel` is never in the document, so the only case it
  measures is the one where nothing competes for the width. **Should be:** the
  script selects a step before measuring, at every width.

## 7. The empty, the loading and the fault not drawn

**FOUND. This is the largest single piece of work in the window.**

- **Scene:** `now-375`, `now-1440`. **Element:** the whole main area.
  `<p class="now__mute">I cannot tell you what waits: outside the desktop shell
  there is no engine to ask</p>`, set flush at the top-left of eleven hundred
  empty pixels. No container, no icon, no vertical centring, no retry, no
  explanation of what would fix it. **Should be:** a centred block with a title,
  the sentence as body, and the one action that resolves it.
- **Scene:** `installed-375`, `installed-1440`. **Element:** the whole main area.
  **Two** refusals stacked — *"I cannot sweep this machine"* and *"Cannot take
  stock of this machine"* — in identical prose, so the reader cannot tell whether
  that is one problem or two.
- **Scene:** `history-375`, `history-1440`. **Element:** under the four tabs. Two
  more refusals in the same shape.
- **The loading state is not photographed at all.** Every capture is either
  loaded sample data or a refusal; nothing in the twelve shows `asking`.
- **The fault state has no drawing of its own.** The broken flow `notte` renders
  its serde error as raw red monospace in the rail (sign 2), which is the only
  fault the twelve captures contain.

## 8. A component library installed and never used

**FOUND, and it is nearly total.**

`package.json` installs `shadcn@^4.20.1`, `radix-ui@^1.6.7`,
`class-variance-authority`, `clsx`, `tailwind-merge` and `tw-animate-css`.
`desktop/src/components/ui/` holds exactly **four** components: `button.tsx`,
`dropdown-menu.tsx`, `tabs.tsx`, `tooltip.tsx`. Each is imported **once** in the
whole application:

```
1 from "@/components/ui/tooltip"
1 from "@/components/ui/tabs"
1 from "@/components/ui/dropdown-menu"
1 from "@/components/ui/button"
```

Everything else — the rail rows, the inspector fields, the add-step toolbar, the
flow chip, the empty states — is hand-written markup against a 7,001-line
stylesheet. **The bundle carries the library's weight and the screen shows four
uses of it**, which is also where the 27.66 kB the bundle check is over is worth
looking first. **Should be:** either the empty/error state, the dialog, the
select and the input come from the library and the hand-written equivalents go,
or the library goes and the four call sites are inlined.

## 9. One more, not on the list, and it is visible in every canvas capture

- **Scene:** `flows-canvas-1440`, `flow-in-focus-1440`, `step-selected-1440`.
  **Element:** the React Flow zoom controls, bottom-left. **Four buttons render
  as four blank rounded squares with no glyph inside them.** The cause is in the
  sheet: `--xy-controls-button-color-default: var(--ink)` (`styles.css:6594`)
  sets `color`, and React Flow paints its control glyphs with `fill`, which no
  rule in the sheet sets. There is no `.react-flow__controls` rule anywhere in
  7,001 lines. **Should be:** `.react-flow__controls button svg { fill:
  var(--ink); }`, or the controls are replaced by the product's own.
