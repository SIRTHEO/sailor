# Sailor runs on every engine, and develops itself

**02/09/2026.** The specification behind the next one-shot mandate. It is the
stopping condition: when every claim below is yes, with its proof, the work is
done. It is written the way `2026-09-02-the-mvp-that-ends-orca.md` was, because
that form ended a build that had iterated for weeks.

Where it comes from, in Theo's words of the same evening: *the same flow we do
in this terminal must be dispatched; this terminal monitors, learns, and
develops Sailor with Sailor, the way it will have to when we detach. Not to
dodge limits: to use the tokens the providers give. A product that heals,
fixes and develops itself. A window centred on the AI that lets a person see,
understand, act and maintain, event-driven. Terminals are focal because the
person must still work with the AI.* And, an hour later: *a system with many
terminals and agents that knows itself very little; agents of different
command lines working on one goal do not know who does what.*

## What is already true, measured on this machine

Starting point, so the mandate builds on it instead of beside it.

- **Fuel.** `sailor remaining` reads the person's Claude quota from the OAuth
  usage channel: five-hour and seven-day windows with their reset. The other
  three engines declare `read_remaining_quota: false` or say nothing. The
  descriptors carry `unusable_when` (claude, codex, agy) and `usage` readers
  (claude, codex, agy); gemini has neither, on purpose. `sailor models list
  --free-only` lists the free models of the catalogue with context and price.
  `crates/models/src/config.rs::effective_model` still enforces the free-only
  rule of 27/08: no paid model can be chosen by configuration.
- **Who actually worked, in the ledger.** Twenty-eight model calls between
  28/08 and 01/09: nineteen by Claude on `claude-opus-5`, 37.89 $; four by
  codex and one by agy with no cost known; none by gemini, ollama or
  OpenRouter. Both codex profiles are not authenticated; agy is present at
  1.1.23; ollama runs but holds only two embedding models and no model that
  answers; no OpenRouter key is set. Theo pays for four command lines and
  one does the work: this is the sentence the fuel group exists to make
  false.
- **Dispatch.** `dispatch-the-work` (built in, six steps) splits a mandate in
  two, runs two engines in parallel, has a third verify, and ends red or green.
  The engines are named by tool id and resolved on the launcher's machine.
  `smista-il-lavoro` and `sviluppa-sailor` (twelve steps) exist in the home.
- **Coordination.** `work_claim`, `work_release`, `work_survey` exist as
  actions with leases, renewal, three collision kinds and a survey that
  separates the living from the gone (`crates/actions/src/presence.rs`). No
  command line calls them: the hooks Sailor grafts (`sailor session
  open|event`) record terminals, not claims.
- **Self-development.** The three gate rings are written
  (`2026-08-28-sailor-si-sviluppa-su-se-stesso.md` §2.2); the flow
  `autocura-dei-flussi` was validated by the engine and never entered the tree;
  the apply-patch action, the compiled deny list and the canary test do not
  exist.
- **Workspaces.** `sailor.json` marks a root; the register exists
  (`sailor workspace list`) and on this machine says «no project has been
  opened yet»; the window can move into a project (`work_here`).
- **Window.** Four places, the bar with its chips, a SQL box on the ledger,
  «what Sailor keeps» with real paths and since when, a grid of terminals with
  a router line, Stop, handed steps, dark scheme, English throughout. The six
  gaps of the judges are listed in `2026-09-02-la-finestra-che-mostra-se-stessa.md`.
- **Orca on this machine.** Ten hook lines in `~/.claude/settings.json` still
  call `~/.orca/agent-hooks/claude-hook.sh`; `~/.orca` holds 64 KB (hooks,
  agent-teams binary, Linear tokens); `settings.json.prima-di-sailor` is the
  copy from before the graft; five git worktrees of this repository; five
  GitHub repositories, two archived. The built-in flow `migrate-to-sailor` has
  four steps.

## A. Fuel: every engine says what it has left, before it is chosen

1. **Every engine that can be read is read before the choice.** The
   descriptor declares a `quota_probe`: Claude's OAuth usage channel (known);
   Codex's app-server `account/rateLimits/read` (`usedPercent`,
   `windowDurationMins`, `resetsAt`, cached because it costs seconds to
   start); a provider endpoint for keyed engines (OpenRouter `GET
   /api/v1/key`, the `x-ratelimit-remaining-*` headers of Groq and
   SambaNova). Gemini has no machine channel and keeps `unusable_when` only,
   and its descriptor is re-verified: the free path through Google login
   ended on 18/06/2026. `sailor remaining` prints one line per engine and
   profile, and «cannot read» is a line of its own, never a zero.
   *Proof:* a test per reader on a recorded answer; the absurd control first: a
   refusal is never an empty measure (already the rule in `remaining.rs`).
   Sources: `2026-09-02-how-others-run-on-many-engines.md`.
2. **Exhaustion is a class of its own, with the reset.** `quota_exhausted`
   joins `ENGINE_FAILURES`; the forms that mean it are declared per engine in
   the descriptor, not in code; the reset time is extracted when present. A
   chain moves to the next engine knowing why, and the ledger row says it.
   Beside the profile, a cooldown state by error class (`cooldown_until`,
   `error_count`, `disabled_until`, `disabled_reason`, the form OpenClaw
   keeps): an exhausted engine is not a broken engine, and it comes back at
   its reset without anybody touching a file.
   *Proof:* a fake engine that prints the weekly-limit sentence closes the step
   with that class; removing the form from the descriptor turns it back into
   `exit_error`.
3. **A budget per profile on a declared window excludes, never reorders.**
   Declared in a file beside the profile store; summed from `model_calls` of
   the active profile; over the cap the engine leaves `candidates` as
   `Refused { unresolved: false }` with the reset day in the reason.
   *Proof:* two calls under a cap of one, the second is refused and the chain
   goes on.
4. **The catalogue carries what nobody's does: how much is free, on what pact,
   how much is left.** Three columns in `crates/models`, fed by the OmniRoute
   free-tier dataset (MIT, decision of 30/08) and by A.1. Every free entry
   carries `data_pact: trains | does_not_train | unknown`, **per model, not
   per provider** (OpenRouter and OpenCode Zen declare it per model), and
   `unknown` is not `does_not_train`. No number from a blog enters: Cerebras
   and NVIDIA showed what blogs get wrong.
   *Proof:* the catalogue refuses to load an entry that names a pact outside
   the three words.
5. **Not every job may go everywhere.** A step declares `data: private |
   public`; a private step never resolves to an engine whose pact is `trains`
   or `unknown`. The refusal names the pact.
   *Proof:* a flow with a private step and only free-that-train candidates
   stops red naming the pact; flipping the step to public lets it run.
6. **The free-only rule is decided, not left.** Theo answers one question:
   does `effective_model` keep forcing free models? Until he answers, A.4 and
   A.5 land as data and the rule stays. Whatever he decides is written in
   `decisioni.md` with the date.
7. **Every company's models do what they are strongest at, by data.** A step
   declares its `kind` (`mechanical`, `research`, `implementation`,
   `judgement`, `writing`), and a strengths table in `crates/models`, a file
   and not code, maps a kind to an ordered list of candidates across
   companies: a local ollama coder model first for `mechanical` (renames,
   sweeps, translations), the free OpenRouter models for `research` reads, the
   paid subscriptions for `implementation` and `judgement`. The table carries
   the measure it was written on: the ledger's calls per kind with their
   outcome and cost, read by `history_ask`. A kind without a row falls to the
   chain as written, never to Claude by default.
   *Proof:* a `mechanical` step with ollama present resolves to ollama; with
   the row removed it resolves to the chain's first; the ledger names the
   kind on the call.
8. **A subscription window is spent, not saved.** For every engine with a
   window (Claude's five hours and seven days, Codex's the same), the
   dispatcher reads what is left and the time to reset, and prefers the
   engine whose window would otherwise expire unused. At the end of a week,
   the report says how much of each paid window went unused while work
   waited; the number this claim moves is that one, from unknown to measured
   and then down.
   *Proof:* two engines, one at 10 % left resetting in an hour and one at
   80 % left resetting in six days: the dispatcher picks the first for a job
   that fits, and the why says «expires unused».

## B. Dispatch: the same flow, from three seats, chosen by what is left

1. **`dispatch-the-work` chooses by fuel.** The dispatch step receives the
   remaining of every candidate (A.1) and the pact (A.4) and writes
   `first_engine`/`second_engine` with a `why` per choice; the ledger keeps the
   why on the run. A choice without a why is out of shape.
   *Proof:* the run record carries both whys; a dispatch answer without them
   fails the shape.
2. **Three seats, one ledger.** The same flow is started from this terminal
   (a Claude Code session under the Sailor hooks), from a Sailor pane in the
   window, and from a bare `sailor flow run` in a shell; the three runs are
   three rows in `runs`, each with its `started_by` naming the seat, and the
   window's Now shows all three while they run.
   *Proof:* a test on `started_by` for the three values; the gesture is
   Theo's (see the end).
3. **Two engines, two trees.** Each engine of the parallel front works in its
   own git worktree of the project, named after the run and the step, and the
   verify step reads both trees. No two engines share a tree.
   *Proof:* a run leaves two worktrees named `<run>/<step>`; `git worktree
   list` shows them; a flow with the trees disabled is refused.
4. **The judge is blind by construction.** The verify step runs in a fresh
   session (never `--continue`), receives only the mandate and the two
   answers, and its verdict is a closed word. The descriptor's `fork_session`
   is never used for it.
   *Proof:* the command line composed for verify carries no resume flag; a
   test on `command_line(&recipe)`.
5. **The mechanical work goes local, and is judged by a check.** The ollama
   descriptor gains an `ask` block (`ollama run <model>`, prompt on stdin,
   `usage` from `--verbose`), and a flow `sweep-the-tree` takes a mechanical
   mandate (an English sweep, a rename, a comment cut), runs it on the local
   coder model file by file in a worktree, and closes green only on
   `cargo test` and the ratchets, never on the model's word. Claude is not a
   candidate of that flow.
   *Proof:* the run on this repository for one of the ratchets, with zero
   Claude calls in its ledger rows.

## C. Sailor develops Sailor, and this terminal watches

1. **The three rings hold.** Ring 1: no self-care step has write powers; a
   proposal is a diff as data in an `answer_shape`. Ring 2: an `apply_patch`
   action in Rust, with a compiled deny list (the authorisation file, the
   apply module, its test) that no data can widen, and an allow file in
   `$SAILOR_HOME/autocura.json` outside the tree. Ring 3: the step that hands
   the patch is `HandToHuman`, taken and closed from the window.
   *Proof:* the canary test in `crates/sailor/tests/`: a patch that touches the
   deny list is refused even when the allow file names it; deleting the deny
   list from the binary turns the canary red.
2. **`sviluppa-sailor` runs under Sailor on Sailor.** The twelve-step flow
   runs from the window on this repository with a real mandate (one of the
   six judge gaps), the implement step in its own worktree, the verify step
   blind, the patch handed to Theo. The run is a row in the ledger with cost.
   *Proof:* the run id, its cost, its handed step, in the outcome section of
   this document.
3. **A broken run becomes a fault, by flow.** A flow reads `history_ask` for
   the last broken run of every flow due today and writes the fault with the
   check that would have caught it; a fault already present is not written
   twice.
   *Proof:* two runs of the flow on the same broken run leave one fault.
4. **This terminal is the watch.** A flow `watch-the-crew` reads
   `work_survey`, the tracked sessions and the open runs, and prints who does
   what, where, since when, and what died; the Claude Code session under the
   Sailor hooks runs it at each beat and says when something is due, without
   a person asking.
   *Proof:* the flow's output on this machine, with the sessions of tonight.
5. **A loop stops for three reasons and one wall.** A self-care run ends on
   a declared promise, on a number of turns, or by hand; and every run
   carries a wall of time the model is told about (Copilot warns at fifty-nine
   minutes; OpenHands shows the harm of a limit the model does not know). Each
   closed self-care run leaves one ledger line in the shape of autoresearch:
   commit, the metric it moved, `keep | discard | crash`, one sentence.
   *Proof:* a run past its wall closes `stopped` with the wall as reason; the
   line is read back by `history_ask`.

## D. Self-knowledge: who does what, and a memory that is read back

1. **Every tracked terminal claims.** The `sailor session open` hook also
   writes a `work_claim` (agent = the command line and its profile, workdir,
   branch, paths empty) and renews it at each event; `close` releases. Agents
   of different command lines see each other in `work_survey`. The claim
   carries the shared words so an export is free later: the OpenTelemetry
   agent names (`gen_ai.agent.name`, `gen_ai.agent.id`,
   `gen_ai.conversation.id`) and the A2A task states (`working`,
   `input_required`, `completed`, `failed`). And the survey is **injected at
   every agent's start**: the Claude SessionStart hook adds it as context, and
   Codex and Gemini read it from the generated file of D.3.
   *Proof:* two sessions of two command lines opened in one tree show one
   `same_workdir` collision in the survey, and the second session's first
   context names the first.
2. **The window shows the crew.** Terminals › crew lists every claim: who,
   which program and model and profile, which tree and branch, doing what,
   since when, and the gone with their why. It is the survey drawn, not a
   second source.
   *Proof:* the screen reads `work_survey` and nothing else; a test on the
   command asked.
3. **The ledger is the memory, without a vector store.** A `memories` table
   with `type` (user, feedback, project, reference), `label`, `value`,
   `provenance` (run, step, session), `modified`, and `valid_from` /
   `valid_until` instead of deletion; written only by a step of surface
   `remember`. An FTS5 index over runs, steps, events, faults, the store and
   the text of every `.flow.json`, asked from a flow (`flow_search`,
   `memory_search`) and from the window's SQL box. A `MEMORY.md`-equivalent
   of at most 200 lines is generated from the table and handed to all three
   command lines from one file (`@AGENTS.md` for Claude, `AGENTS.md` for
   Codex, `context.fileName` for Gemini) through `launch.env`. A periodic
   flow consolidates raw memories into summaries with a reviewable diff, and
   redacts secrets before the first write. Embeddings join only when a named
   question fails on FTS5, and then in the same file with sqlite-vec.
   *Proof:* searching a word that appears only in one flow's prompt finds
   that flow and no other; a memory with a secret never reaches the table;
   the generated file is byte-identical for the three command lines.
   Sources: `2026-09-02-memory-self-knowledge-and-specialists.md`.
   *Proof:* searching a word that appears only in one flow's prompt finds that
   flow and no other.
4. **What Sailor remembers of a run is written down, once.** The report of
   every closed run (what went, what broke, what it cost, what was learnt) is
   a store record, and a flow that starts on the same entity reads the last
   report first.
   *Proof:* a second run of a flow carries the previous report in its trigger.
5. **Skills, rules and equipment are Sailor's to manage.** `sailor
   inventory` already lists skills, agents, commands, rules and hooks and
   says which are off. A flow reads it per workspace, proposes what to
   switch on or off and what is stale against the workspace's `sailor.json`,
   and hands the proposal as a choice with closed options; applying it is
   `apply_patch` under ring 2, never a shell.
   *Proof:* a workspace declaring a rule file that is absent yields one
   proposal; the same run twice yields one.
6. **The specialists are forced, not suggested.** Sailor generates
   `.claude/agents/*.md` from its descriptors and roles (explorer, judge,
   researcher, implementer); a coordinating session starts as `claude
   --agent` with `tools: Agent(<the list>), Read, Bash` and **without Edit or
   Write**, so it cannot implement; where a flow declares `gate`, a
   PreToolUse hook exits 2 on a tool outside the list. For Codex and Gemini
   the same is declared uncovered until their command lines offer a wall.
   *Proof:* the coordinator asked to edit a file is refused by the platform;
   removing `Agent(...)` from the generated file leaves it unable to spawn.

## E. The window: workspaces first, terminals with their agents, one event stream

1. **A workspace is a page.** Board shows one workspace at a time, its flows
   and its runs; the rail names the workspace; system flows and home flows are
   two pages of their own; the switch moves root, terminals' default tree and
   credentials together.
   *Proof:* two projects registered, the board changes flows on switch; a test
   on `flowPlaces` per root.
2. **A terminal says who runs in it.** The pane header shows the program, the
   model in force, the profile, the tree, and the estimated tokens against the
   ceiling; a pane that runs no agent says «shell».
   *Proof:* a pane test on the four words.
3. **One event stream, from anywhere.** Runs, sessions, claims, faults and
   the build feed one `sailor_event` channel; the bar consumes only that
   channel; Now is a view of the same events. A poll that fails is shown red
   with its reason.
   *Proof:* a test that the bar subscribes to one channel and asks nothing
   else; the mute state on a refused poll stays.
4. **Theo is asked only for choices.** Every question the system puts to a
   person is a `HandToHuman` step with closed options and the facts each
   option rests on; free-text questions do not exist in flows. The bar
   counts the choices waiting; Now lists them first; the same list is
   printed by `sailor step list` for a terminal. What is not a choice is done
   and reported.
   *Proof:* a handed step without options is refused by `flow check`.
5. **The six gaps of the judges are closed or deprecated with a reason.**
   Bar carries three facts only; Sailor › models is a catalogue with «in use»;
   can do shows surface and powers; keeps shows growth and identities; the
   band shows state and cost per flow; the ledger keeps a run's root.
   *Proof:* the judges' list in `la-finestra-che-mostra-se-stessa.md` marked
   one by one.

## F. Orca leaves the machine, by flow

1. **`migrate-to-sailor` does the uninstall.** It takes the ten Orca hook
   lines out of `~/.claude/settings.json` (keeping the Sailor grafts), moves
   `~/.orca` to the archive with the date, lists the projects under
   `~/.claude/projects` that no tree on disk matches, and names the GitHub
   repositories not touched since a declared date; it deletes nothing that is
   not a hook line, and hands the list to Theo.
   *Proof:* a run on a scratch home with a fake settings file leaves the
   Sailor lines and removes the Orca ones; the archive carries the date.
2. **The number.** The document's outcome section says how long the migration
   took on this machine, in minutes of Theo's gestures and minutes of flows.

## Out of scope, deliberately

- No proxy, gateway or resident server that translates provider formats
  (decision of 30/08). Endpoints that speak a command line's native protocol
  are pointed at through `launch.env`; nothing is translated here.
- No impersonation of another product's OAuth client or user agent, ever.
  Subscription command lines run as the unmodified binary Sailor launches,
  which is the case Anthropic's terms admit; nothing sits between the binary
  and its provider. Rotation is across different providers and the person's
  own subscriptions, never across several accounts of one provider.
- No automatic router that scores engines: the chain order is written by the
  flow's author; fuel and pact can only remove candidates.
- No new vector store until structured search fails a named question: the
  research of the same night measured agentic keyword search at 91–95 % of
  RAG, and the three command lines keep their memory in files.
- No always-on worker, MCP server, Docker container or external database for
  memory or coordination: the ledger is the tracker, and a flow re-reads it.
- No new place in the window beyond the four.

## The order

A, then B, then C, then D, then E, then F. A.6 is asked of Theo at the start
and does not block A.1–A.5. Inside a group the order is free.

## The termination condition

Every claim above is yes with its proof named, or no with the measured reason
and a line in `da-fare.md`; the outcome section below is written; the binary
in service is released from the last commit and copied where the hooks call
it; the seven gestures below are listed for Theo.

## What is still Theo's: the gestures

1. Answer A.6.
2. Give the engines their credentials: log the two codex profiles in, keep
   agy logged in, set an OpenRouter key in the profile's environment, and
   pull one coder model on ollama (the mandate names which). Credentials
   are the one thing no flow can do.
3. Run `sailor remaining` and read one line per engine.
4. Start `dispatch-the-work` from this terminal, from a pane, from a shell;
   watch the three in Now.
5. Take the handed patch of `sviluppa-sailor` from the window and close it.
6. Open Terminals › crew with two command lines in one tree.
7. Switch workspace from the rail and watch the board change.
8. Run `migrate-to-sailor` and read the list it hands over.
