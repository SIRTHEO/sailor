# The charter, checked against the render and against the sheet

The twelve prohibitions at the head of `desktop/src/styles.css` (lines 40–72),
each read against the rendered screenshots and against the 7,001 lines below
them. Three columns of verdict: **holds**, **declared but broken**, **nothing
enforces it**.

## The table

| # | prohibition | in the sheet | in the render | a check? |
| --- | --- | --- | --- | --- |
| 1 | two faces, none by omission | **bent** | holds | partial |
| 2 | no radius outside three | **bent** | holds | yes, and it is blind to the bend |
| 3 | one shadow only | holds | holds | yes |
| 4 | colour reserved for state + one accent | holds | holds | yes |
| 5 | colour never the sole carrier | holds | holds | partial |
| 6 | no pair below 4.5:1 | holds | holds | yes, and it walks the DOM |
| 7 | two text levels, not three | **broken** | broken | **no** |
| 8 | scale, 4 px grid, nothing under 12 px | holds | holds | yes, three tests |
| 9 | no gradient, no blur, no pill button | **broken** | not visible | partial |
| 10 | compared numbers in tabular figures | holds | holds | no |
| 11 | a fixed column says what it does when narrow | holds | **broken** | yes, **and it is blind** |
| 12 | the ground is never pure black, never neutral | holds | holds | no |

## What holds, and is proved

**3 — one shadow.** `box-shadow` appears six times. Four are `var(--shadow)`, two
are `inset 0 0 0 1px var(--focus)`, which the test itself names: *"the only
shadow is `--shadow`; an inner ring is not a shadow"*. `--shadow` is redefined at
lines 281 and 330, but both are the other colour scheme, not a second shadow.

**6 — contrast.** This is the best-defended rule in the sheet.
`contrast.test.tsx` does not read tokens, it **walks the painted DOM under both
schemes** and rejects any pair below 4.5:1, across the opening view, the run
history and the reachability states. It is the one check that would catch
lightening `--muted` by two characters.

**8 — the scale and the floor.** `--text-small: 12px`, `--text-body: 13px`,
`--text-mid: 14px`, `--text-large: 18px`, with `--text-micro` and `--text-display`
declared as aliases of the first and last. **Nothing is under 12 px.** Spaces are
`--space-1..7` = 4, 8, 12, 16, 24, 32, 48. Three tests defend it: *"no size
written by hand"*, *"the scale itself has nothing under the floor"*, and *"and no
rule in the sheet declares a size under the floor"*.

**12 — the ground.** `--surface-0: #19171d` (25, 23, 29), `--surface-1: #211e26`,
`--surface-2: #2b2731`. Never black, and blue exceeds red exceeds green in each:
the plum undertone is really there. Visible in all twelve captures. **No check
enforces the undertone** — only prohibition 4's colour-role tests, which say
nothing about hue relations.

## Declared but broken

**7 — TWO text levels, not three.** The sheet's own `:root` declares
**`--text-1`, `--text-2` and `--text-3`** (lines 86–88), redefined for both
schemes at 264–266 and 313–315. `var(--text-3)` is used three times. The
prohibition says two.

**And the check for prohibition 7 does not check this.** The test named
*"prohibition 7 is written into the sheet, not only into the comment"*
(`stylesheet.test.ts:268`) asserts one thing: `--faint` equals `--muted`. That was
a real abolition of a fourth level and the check earns its place, but **it does
not look at `--text-1/2/3` at all**. The prohibition as written is enforced by
nothing.

**9 — no pill-shaped button.** `--radius-pill` is used 20 times. Nineteen are
badges, dots and marks — `.rail__dot`, `.focusbar__live`, `.now__badge`,
`.step-node__state-dot`, `.speaks` and so on — which is exactly the exception the
prohibition grants. The twentieth is `.sketch__arrow` (`styles.css:6661`), which
carries `cursor: pointer`, a border and padding, and is applied to an element in
`Sketch.tsx:138`. **That is a pill-shaped button.** The check for prohibition 2
admits `var(--radius-pill)` everywhere, so it cannot see the difference between a
badge and a button; no check knows which elements are buttons.

Not visible in the twelve captures: `Sketch` is not one of the six scenes.

**1 — no font chosen by omission.** The prohibition bans Roboto, Arial,
**Helvetica**, Open Sans, Lato, Montserrat, and `system-ui` first. The three role
tokens are `--font-display` and `--font-prose`, both
`"Inter Variable", Inter, "Helvetica Neue", sans-serif`, and `--font-data`.
Helvetica Neue is third in the stack, not first, so nothing chosen by omission
reaches the screen while Inter loads — but the banned name is in the sheet.

**The check cannot see it.** *"No font stack written by hand outside the roles"*
filters `font-family` **declarations** and requires each to be
`var(--font-display|prose|data)`. It never reads the value of those three tokens,
which is the only place a banned family could ever enter.

**2 — no radius outside three.** The prohibition names `--radius`, `--radius-lg`,
`--radius-pill`. The sheet declares four more at 6560–6566: `--radius-sm`,
`--radius-md`, `--radius-xl`, `--radius-4xl`. They are all aliases of the three
allowed values, so no fourth radius reaches the screen, and the check — which
reads `border-radius` declarations — passes correctly. But the shadcn block
introduced four more names for three roles, which is the shape the prohibition
was written against.

## Broken in the render, and the check for it is blind

**11 — a fixed column says what it does when the window is narrow.**

The prohibition is written from a real injury: *"at 375px the rail and the
inspector came to 520 and the canvas — the surface this window exists for — was
zero pixels wide."* Two checks defend it. `stylesheet.test.ts` reads the cause out
of the sheet, carefully, and even documents why a 9 px dot is not a column.
`npm run check:canvas` measures the drawn geometry at 375, 760, 1100 and 1440.
Both are green.

**And `step-selected-375` shows the injury happening.** The inspector holds
roughly 255 of 375 CSS px; the canvas gets about 87; the add-step toolbar renders
as `TRIGG`, `AGEN`, `CHEC`; the flow description truncates as
`Resets a full session and h`.

The reason the check does not see it is in its own output, in every row:

```
✓   375px  rail null · canvas  375 · panel null   nodes in view: 1/1
✓  1440px  rail null · canvas 1200 · panel null   nodes in view: 1/1
```

`panel null`, four times. **The script never selects a step**, so the inspector is
never in the document, so the only case ever measured is the one where nothing is
competing for the width. The check is not wrong about what it measured; it has
never been shown the case the prohibition was written about.

I did not edit the script — it is not a path I own. The change it needs is one
line: click a step node before taking the measurement, at every width.

## Enforced by nothing at all

- **Prohibition 7**, as argued above. Its named check tests a different pair.
- **Prohibition 10** — numbers compared across rows in `--font-data` with tabular
  figures. `font-variant-numeric` appears 16 times and the step names, counts and
  ids are all monospace in the captures, so it holds in practice. No test asserts
  that a column of compared numbers is in the data face.
- **Prohibition 12** — the plum undertone. Holds by measurement of the three
  ground tokens; nothing would go red if somebody made them neutral grey.
- **Prohibition 5** — colour never the sole carrier of a state. It holds
  everywhere in the captures: `CHECK ● WENT`, `● RUNNING`, `not run yet` — the
  word always sits beside the tint. Individual component tests
  (`StepNode.test.tsx`, `ports.test.tsx`) assert the word for their own node,
  but no sheet-wide check says a tinted thing must carry a word.

## The one sentence

The sheet defends colour and size better than any project this size usually
does, and **the two prohibitions that are actually violated today — 7 in the
tokens and 11 in the render — are both violated in the blind spot of the check
written for them.**
