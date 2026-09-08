# The contract of the terminal in the window

**01/09/2026.** Written before the work and not after, because two building
sites build it in parallel — the bridge (Rust, inside `desktop/src-tauri`) and
the window (React, inside `desktop/src`) — and without a written contract they
would meet only at the merge. Fault 31 was born exactly like that: nine agents
on the same repository, four conflicts `git` could not see because they were
conflicts of intent, not of lines.

**This file is the source for both.** Whoever changes the contract changes it
here and says so; whoever finds it differing from the code opens a fault instead
of quietly bringing their own half into line.

## Why the building site exists

`crates/terminal` is finished and tested — 2,295 lines: a real pseudo-terminal,
writing, output while it comes out, resizing, closing, the list of what is open,
and the routing of what the user types, treated as data (`routing`). **No binary
ships it**: the list of commands the window exposes
(`desktop/src-tauri/src/main.rs`) does not hold a single one concerning a
terminal.

Without a terminal in the window, an engine session cannot be held open inside
Sailor: it is the piece that decides whether Sailor can replace the environment
the owner works in today.

## The names, and why they are in English

They are identifiers — the compiler reads them and `invoke(...)` writes them —
so English, as `AGENTS.md` says. What a person reads stays Italian.

## The commands the bridge exposes

Seven since 02/09/2026 (there were six: `terminal_backlog` came in with survival
of the window), and not one more without updating this file.

| command | arguments | answer |
|---|---|---|
| `terminal_open` | `{ workspaceRoot: string, program?: string, args?: string[], cols: number, rows: number }` | `{ id, workspaceRoot, workspaceName, alive, processId, device, moved }` |
| `terminal_submit` | `{ id: string, line: string }` | `{ kind: "command" } \| { kind: "flow", flow: string, text: string, rule: string }` |
| `terminal_press` | `{ id: string, bytes: string }` (base64) | `null` |
| `terminal_resize` | `{ id: string, cols: number, rows: number }` | `null` |
| `terminal_close` | `{ id: string }` | `null` |
| `terminal_list` | — | `[{ id, workspaceRoot, workspaceName, alive, processId, device, moved }]` |
| `terminal_backlog` | `{ id: string }` | `{ at: number, bytes: string (base64), upto: number, ended: string \| null }` |

`device` is the tty of the program inside, in short form (`ttys004`): it is the
anchor of a tab, the key of the mailbox and of the count. `moved` is the bytes
passed so far in both directions, the same number `sailor terminal list` prints.
`terminal_open` opens under the active profiles: the environment of the process
inside carries `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and the other variables that
`profiles::active_environment` draws from the profile store at the moment of
opening.

`terminal_backlog` serves what the terminal printed before this panel was
looking, up to a declared limit (`terminal::host::BACKLOG_LIMIT`), and `upto` is
the offset from which the live events take over: a panel writes the backlog and
then only the events whose `at` is not below `upto`.

The row of the list is `terminal::Summary`, which is already `Serialize`: **a
type Rust already declares does not get copied out again in TypeScript** — it is
fault 10, which in this repository has already come back five times.

Every command returns `Result<_, String>`: the error is the text `PtyError`
produces, and it goes in front of whoever is looking instead of ending up in a
`console.error`.

### Why `submit` and `press` are two commands and not one

The engine already tells them apart, and the distinction is the point of the
whole crate: `submit` looks at a whole line **before** running it and can send
it to a flow instead of to the shell; `press` passes raw bytes — a Ctrl-C, an
arrow key, the answer to an interactive question — without having them examined
by a set of rules that has nothing to do with them.

The window sends to `submit` only what the user confirms with Enter at the start
of a line; everything else is `press`. An emulator that sent everything to
`submit` would put every keystroke through the routing, and an editor inside the
terminal would become unusable.

## The output event

Name: **`terminal_output`**. Payload: `{ id: string, bytes: string, at: number }`,
where `bytes` is base64 and `at` is the offset of the first byte from the
opening of the terminal — what lets a panel join the backlog and the live output
with neither a hole nor a repetition.

**Base64 and not a string** because what comes out of a pseudo-terminal is a
sequence of bytes that can break in the middle of a multibyte character:
handing it over as a string would corrupt it, and the vanished accent would show
up only on an Italian word in the middle of a long output.

An event **`terminal_closed`** with `{ id, status }` says the process inside has
finished: without it, the window would show a dead terminal as alive — which is
the shape fault 12 comes back in every time.

## Who owns which files

So that two parallel building sites do not touch each other:

- **the bridge**: `desktop/src-tauri/src/terminal.rs` (new), `crates/terminal/**`,
  `crates/supervisor/**`, and **one single line** in
  `desktop/src-tauri/src/main.rs` — the six entries inside `generate_handler!`;
- **the window**: `desktop/src/Terminals.tsx`, `desktop/src/TerminalPane.tsx`,
  `desktop/src/terminal.ts` and their tests (new), `desktop/src/styles.css`, and
  **a few lines** in `desktop/src/App.tsx` for the navigation entry.

Whoever needs to touch a file of the other's stops and says so to whoever is
coordinating.

## What was missing in the crate — corrected after the measurement

**This paragraph said something false, and is to be read as history.** It asked
the bridge for «the way to read the output», because `Terminal` does not expose
`Pty::reader()`. But the thing it was needed for **was already there**:
`Terminals::open` took an `Arc<dyn Output>` and already opened the thread that
drains, and the test that the first piece arrives before the second one exists
had been in the repository since before this building site.

What was really missing is **the ending**: nobody said the process inside had
finished, and without that the `terminal_closed` event this document demands
could not have existed. It is what the bridge built — `Ending` with its three
cases, `Output::ended`, `Pty::finished()` which never blocks.

The correction sits here and not in a code comment because the document asks for
it in plain words: *whoever finds it differing from the code opens a fault
instead of quietly bringing their own half into line*. That holds for whoever
wrote the document too.

## The two properties the building site has to have, and how they show red

1. **A terminal outlives the window.** Whoever closes the window does not kill
   the session inside: on restart, `terminal_list` finds it again and the window
   hooks back on to it.

   **True since 02/09/2026, and by the road the paragraph below pointed to as
   the necessary one: a resident process holding the ends of the ptys.** It is
   `sailor terminal host` (`crates/terminal/src/host.rs`): the bridge opens no
   pseudo-terminal at all, it is a client of that process over a socket next to
   the mailboxes, and it starts it if nobody answers. The test is
   `crates/sailor/tests/a_terminal_outlives_the_window.rs`, with the real
   binary: the client that opened the shell disappears, a new client finds it
   alive with its backlog, and the absurd check — kill the host, the shell dies
   — says the pty belongs to the host and to nobody else. What follows is the
   history of how it was measured before.

   **It is not done by going through `supervisor::child::Process::start`, and
   this document said the opposite.** Measured on 01/09, and confirmed by a
   judge who had not written the bridge: that road does `Command::spawn()` with
   `Stdio::null()` — no pseudo-terminal, no `setsid`, no `TIOCSCTTY` — and in
   the whole of `crates/supervisor` there is not one line that opens a pty. It
   stays true that it is **the only road that records**, and that going around
   it is forbidden (it is fault 4). So the property does not belong to this
   building site: it is a building site of its own, and it starts from the only
   lawful road — **teaching `Process::start` to start inside a
   pseudo-terminal**, with one more entry in its `Spec`.

   It also takes a resident process to hold the ends of the ptys: today they
   live in the window's process, and once that is closed the `follower` goes to
   EOF and the shell exits. No recording in the store changes this.
2. **What comes out arrives while it is coming out, not at the end.** The test
   that says it red: a command that prints, waits, prints again — and the
   assertion that the first piece arrived **before** the second one exists.
