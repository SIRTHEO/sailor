# What the window cannot do

The list that decides whether the switch is possible. Each row was settled by
reading `desktop/src/`, not by looking at the twelve captures — **none of the
twelve scenes photographs the terminal at all**, so the screenshots could only
ever have said "absent" about all of it, and they would have been wrong.

## The short answer

| what he does all day | verdict | what settled it |
| --- | --- | --- |
| several terminals in tabs | **in** | `Terminals.tsx:490` |
| terminals in split panes | **half in** | `styles.css:4726` |
| terminals persisted across a restart | **half in** | `whereyouwere.ts:29`, commit `81da1e4f` |
| projects, each with several git worktrees | **in** | `Worktrees.tsx`, `Projects.tsx` |
| switching between worktrees | **half in** | `worktree.ts:23` |
| scheduled automations | **absent** | `engine.ts:141` |
| a command palette | **in** | `Palette.tsx`, `App.tsx:1323` |

Nothing on this list is a wall. The two that would stop a working day are the
**schedule**, which does not exist, and the **restart**, which forgets after ten
minutes.

## Several terminals in tabs — IN

`Terminals.tsx:490` wraps the open terminals in a Radix `Tabs`, one tab per
terminal, with a liveness word beside each name (`Terminals.tsx:517`). The list
is not the window's: it is asked of the engine, and each entry carries the
workspace it was opened in. Above the tabs sit three sections of their own —
`Live`, `Projects`, `Worktrees` (`terminalstabs.ts`).

Closing, resizing, typing and key-pressing are all wired
(`closeTerminal`, `resizeTerminal`, `pressKeys`, `submitLine`).

**What is not there:** no reorder, no rename, no drag of a tab.

## Terminals in split panes — HALF IN

`.terminals__panes` (`styles.css:4726`) is a CSS grid, `grid-template-columns:
2fr 1fr`, `grid-auto-rows: minmax(200px, 1fr)`, `grid-auto-flow: dense`, and
`.pane[data-focus]` takes `grid-column: 1; grid-row: 1 / span 3`. So **more than
one terminal is on screen at once**: the focused one takes the big left cell and
the others stack down a narrow right column. `data-count` collapses it to a
single column for one terminal (`styles.css:4737`).

**What is not there, and it is the whole difference:** the arrangement is fixed
by the stylesheet and **the user cannot choose it**. There is no "split right",
no "split down", no draggable divider, no way to make two panes equal, and no way
to say which terminals share a pane group. The 2fr/1fr is a decision the sheet
took once for everybody. **A person used to picking where a terminal goes will
notice this on the first day.**

## Persisted across a restart — HALF IN

Two halves, and they were built for different lifetimes.

The **terminal itself survives**: commit `81da1e4f` made the host hold a terminal
past the client that opened it, and `a_terminal_outlives_the_window` and
`a_shell_opened_through_the_host_survives_the_client_that_opened_it` are tests
for exactly that. On reopening, the window asks the engine for the list and gets
the live ones back with what they printed
(`what_was_printed_before_anyone_looked_is_served_to_whoever_attaches_late`).

The **place you were standing does not**, for long. `whereyouwere.ts` writes
`{ place, sailorTab, memoryTab, focus, bench, at }` to `localStorage` under
`sailor.where`, and `STILL_WHERE_YOU_WERE_SECS = 600` (`whereyouwere.ts:29`)
throws it away after ten minutes. Its own comment says why: *"it covers a rebuild
and a crash, and nothing longer"*. **Close the window at night and open it in the
morning and you come back to the head of the list**, not to what you were doing.
That is a deliberate choice for a tool being rebuilt under you all day. It is the
wrong choice for a tool somebody works in.

Also not written down: **which terminal was focused**, and the pane arrangement.

## Projects, each with several git worktrees — IN, with one scope limit

**Projects** (`Projects.tsx`): a table of every project Sailor has been opened in
— name, path, when it was last seen, whether its marker has gone — and `workHere`
moves the window into one, after which the flows are read again. A project
declares itself with `sailor workspace init`.

**Worktrees** (`Worktrees.tsx`): lists the trees, **creates** one from a branch
name, **removes** one, and **opens a terminal on it** — and it refuses to take
down the tree the window itself is running in (`worktree.ts:15`). The comment at
the head of the file says what this replaced: *"until now this was git typed by
hand, so nothing Sailor records knew which tree a run had happened in."*

**The limit, and it is the "switched between" half:** `listTrees()` calls
`worktree_list` **with no argument** (`worktree.ts:23`). It is the trees of *the*
repository — the one the window currently stands in. To see another project's
worktrees you first move the whole window into that project with `workHere`.
There is no view of "my four projects and their eleven trees" side by side, and
switching project is a whole-window move rather than a tab. Outside the Tauri
shell every one of these calls rejects rather than inventing a plausible list.

## Scheduled automations — ABSENT

This is the one with nothing behind it.

`engine.ts:141` declares the entire vocabulary the window has for a schedule:

```ts
scheduled: boolean;
```

`TriggerNode.tsx:111` is its only reader, and all it can draw is a sentence:
*"this flow also has a schedule of its own"*. The window **cannot say when**,
cannot say when it last ran or next runs, cannot create one, cannot edit one and
cannot turn one off. There is no screen that lists the schedules on this machine.
`grep -rniE "schedul|cron|recurring"` over `desktop/src` returns the boolean, that
one sentence, a prose comment in `StepHistory.tsx:105`, and otherwise only
`setTimeout` in the scripted demonstration.

The engine side is not the blocker: the trigger already knows a flow is
scheduled, so something upstream holds the real schedule. **The window is what is
missing, entirely.**

## A command palette — IN

`Palette.tsx` (112 lines), opened by ⌘K or ⌃K anywhere (`isPaletteKey`,
`App.tsx:338`) and by the button in the top bar (`App.tsx:1397`). It filters on
every typed word against group, label and hint; arrow keys and Enter; Escape and
a scrim close it. It computes nothing — `App.tsx:1323` hands it the entries, in
six groups:

| group | what it reaches |
| --- | --- |
| `Go to` | every place, plus the memory tabs by name |
| the terminals ground | `Live`, `Projects`, `Worktrees` |
| the machine ground | the nine machine rows |
| `Open flow` | every flow by name, with its origin as the hint |
| `Run flow` | every flow by name — **it runs them, not just opens them** |
| `Demonstration` | the scripted walkthrough |

Its own comment records what it fixed: *"eleven screens behind doors made every
switch several clicks"*, and that the entries deliberately use the same words the
rail uses, because saying `Sailor › Profiles` when the column said `Profiles`
made searching for what you could see find nothing.

**What is not in it:** opening a terminal, creating a worktree, switching
project, and anything at all to do with a schedule. So the three gestures a
working day is made of are the three the palette cannot reach.

## What to build first, if the goal is the switch

1. **The schedule, from nothing.** It is the only row with no implementation at
   all, and a boolean is not a migration path.
2. **`STILL_WHERE_YOU_WERE_SECS`, and what it covers.** Ten minutes is right for
   a rebuild and wrong for a night. It needs a second, longer lifetime that also
   remembers the focused terminal and the pane arrangement.
3. **A split the person chooses.** The grid is already there; what is missing is
   the gesture and somewhere to write the choice down.
4. **Four palette entries**: open a terminal here, cut a worktree, switch
   project, and — once it exists — run or pause a schedule.
