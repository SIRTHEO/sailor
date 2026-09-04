# Sailor runs on every engine, and develops itself

**02/09/2026.** The specification behind the next one-shot mandate. It is the
stopping condition: when every claim below is yes, with its proof, the work is
done. It is written the way `2026-09-02-the-termination-condition.md` was, because
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
   descriptor declares a `quota` block (done on 03/09 for the OAuth usage
   kind: `reader`, `credentials`, `token_pointer`, `url`, `headers`): Claude's
   OAuth usage channel (known);
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
3. **A budget per engine on a declared window excludes, never reorders.**
   Declared in `budgets.json` beside the profile store (`SAILOR_BUDGETS`
   overrides), per engine id: a cap in micro-units and a window in seconds.
   Summed from `model_calls` of that engine since the window opened, across
   every run; at the cap the engine leaves `candidates` as
   `Refused { unresolved: false }` naming the sum, the cap, the window and
   the calls of unknown cost. Per engine, not per profile: `model_calls` has
   no profile column yet, and a cap per profile would have nothing to sum.
   *Proof:* two priced calls under a cap one call fills, the second is refused
   before spending and the chain goes on; a cap on another engine binds nothing.
4. **The catalogue carries what nobody's does: how much is free, on what pact,
   how much is left.** Three columns in `crates/models`, fed by the OmniRoute
   free-tier dataset (MIT, decision of 30/08) and by A.1. Every free entry
   carries `data_pact: trains | does_not_train | unknown`, **per model, not
   per provider** (OpenRouter and OpenCode Zen declare it per model), and
   `unknown` is not `does_not_train`. No number from a blog enters: Cerebras
   and NVIDIA showed what blogs get wrong.
   *Where it landed:* `models::pact` (`DataPact`, `Pacts`, shipped
   `pacts.default.json`, empty until the first pact is read from a provider's
   own terms); `Model.data_pact` written by `Catalog::declare_pacts`; a
   descriptor's `data_pact` for the models its own subscription reaches. The
   OmniRoute import and the «how much is left» column are still to do: the
   second is A.1's, per engine.
   *Proof:* the pacts file and the descriptor both refuse a fourth word,
   naming it.
5. **Not every job may go everywhere.** A step declares `data: private |
   public`; a private step never resolves to an engine whose pact is `trains`
   or `unknown`. The refusal names the pact. A step that writes a bare
   command (`bin`) is not an engine and is not held: the author wrote where
   its text goes.
   *Proof:* a private step against an engine whose pact is `trains` or
   `unknown` is refused before spending, naming the pact; the same step said
   public, or the same engine under `does_not_train`, runs.
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
   *Where it landed:* `models::strengths` with `strengths.default.json`
   (replaced whole by `strengths.json` in the home or `SAILOR_STRENGTHS`);
   `kind` on the step; the table's engines first, then the chain as written;
   `work_kind` on the `model_calls` row. The table says it rests on the
   mandate, not on a ledger measure yet: the sum per kind is still to write.
   *Proof:* a `mechanical` step with a local engine in the row resolves to it
   ahead of the chain; with the row removed it resolves to the chain's first;
   the ledger row names the kind.
8. **A subscription window is spent, not saved.** For every engine with a
   window (Claude's five hours and seven days, Codex's the same), the
   dispatcher reads what is left and the time to reset, and prefers the
   engine whose window would otherwise expire unused. At the end of a week,
   the report says how much of each paid window went unused while work
   waited; the number this claim moves is that one, from unknown to measured
   and then down.
   *Where it landed:* `models::fuel` (`Fuel::from_remaining`, `prefer`, the
   provider's instant read to seconds); a step's `prefer: fuel` moves the
   engine whose window expires unused soonest to the front of its chain,
   and the why is printed on the step's stderr. The weekly «unused while
   work waited» number is still to write: it needs the windows sampled over
   the week, which nothing records yet.
   *Proof:* two engines, one at 10 % left resetting in an hour and one at
   80 % left resetting in six days: the first is preferred and the why says
   «expires unused»; the same chain without `prefer` runs as written.
9. **A command line can run on another company's model, without a proxy.**
   A profile declares an endpoint and the environment variable of its key;
   Sailor launches the unmodified command line with `launch.env` pointing it
   there (`ANTHROPIC_BASE_URL` for Claude Code), and only at endpoints that
   speak that command line's protocol natively: OpenRouter's Anthropic-shaped
   endpoint, a local ollama, a provider that publishes the same protocol.
   Nothing of Sailor's sits in between and nothing is translated. The ledger
   row names the endpoint and the model that answered, so the spend is never
   attributed to the subscription. The idea is free-claude-code's; the fifty
   providers of its list are candidates to verify one by one, never a
   catalogue, and the ones that log in through another product's
   subscription are out.
   *Where it landed:* `Profile.endpoint` (`url`, `key_var`, `protocol`; the
   key itself never in the store), `sailor profiles endpoint <cli> <name>
   <url> <key-var> <protocol>`, the `endpoint` block of the profiles' table
   (`ANTHROPIC_BASE_URL`/`ANTHROPIC_API_KEY` for Claude Code,
   `OPENAI_BASE_URL`/`OPENAI_API_KEY` for Codex), `endpoint_environment`
   refusing a foreign protocol, a missing key or a line without a variable;
   `candidates` refuses the engine with that reason; `ProfileInForce.endpoint`
   on the ledger row. The terminal carries it too, since 03/09: `sailor run`
   refuses the launch when the endpoint cannot be honoured rather than
   starting the command line against the subscription, and a terminal opened
   from the window carries the endpoints of every active profile, with the
   ones that could not be applied travelling beside the environment as
   refusals the window says out loud. Still to do: the model that answered is
   read only where the descriptor's `usage` block reads it.
   *Proof:* a profile with an endpoint makes the composed environment carry
   the two variables and the identity carry the endpoint; a profile whose
   endpoint speaks another protocol is refused naming both protocols.
10. **free-claude-code is connected as a tool, not built in.** Theo, on
    02/09 at night: *let us not exclude it a priori; they are AIs that can
    help, and for the Sailor project I do not mind handing them data.* The
    decision of 30/08 refused to bring its code inside Sailor; connecting it
    as a program on the machine is the other rule of this repository, «a
    node connected, not code of ours». So: a descriptor `free-claude-code`
    (detect `fcc-server`, version, its admin address, its config) like
    docker's or gh's; a profile of Claude Code whose endpoint is that server
    on `localhost:8082` with its token, through A.9; the ledger row names the
    server and the model that answered, read from the answer's `modelUsage`;
    the server's fallback list and tier mapping (`MODEL_OPUS`, `MODEL_SONNET`,
    `MODEL_HAIKU`) stay its own, and Sailor records what came back. The
    workspace declares its data class (A.5): Sailor is `public`, so its steps
    may go there; a workspace that is `private` never resolves to that
    profile. Two things stay out even so: the «connected accounts» that log
    in with another product's subscription are never configured, and the
    server is a program Theo installs and keeps running, never a piece Sailor
    ships or restarts. What Sailor adds is exactly what that project lacks:
    which provider actually answered, at what cost, on what pact, and how
    much was left.
    *Where it landed:* the descriptor `free-claude-code` (detect
    `fcc-server`, `--version`, `~/.fcc`, the install line, `data_pact:
    unknown`), read from the project's README on 03/09 and not measured
    here: the server is not installed on this machine. The way in is A.9's
    profile endpoint, still to verify against the token header the server
    checks (it documents `ANTHROPIC_AUTH_TOKEN=freecc`, and the profile sets
    the key variable). Not done: the workspace's data class, «server down»
    as a refusal before the step, and the decision line in `decisioni.md`,
    which waits for Theo's «vai».
    *Proof:* a step run under that profile leaves a ledger row with the
    server as endpoint and a non-Claude model as `actual_model`; with the
    server down, `candidates` refuses the profile with the reason instead of
    the step breaking; a `private` workspace never composes that command
    line.
11. **No provider is named in code.** Theo, 02/09 at night, on finding
    `read_from_claude` and `== "claude-code"`: *Sailor should be agnostic to
    this; analyse and maintain where else it happened.* The quota reader
    became a kind of channel the descriptor declares (`quota.reader:
    oauth_usage`, credentials, token pointer, address, headers), and the code
    reads kinds, never providers. The rest is a census with a ratchet: a test
    in `crates/sailor/tests/` counts the string literals that name an engine
    (`claude`, `codex`, `gemini`, `agy`, `ollama`, `openrouter`, and the
    descriptor ids) in non-test Rust, is seeded with today's exact count, and
    may only fall. The heaviest places, measured with a rough grep on 02/09:
    the profiles' table of known command lines (22), the actions crate (19),
    `flow_cmd` (16), `run_cmd` (12), `profiles_cmd` (10). The profiles' table
    moves into the descriptors (`home` mechanism, native profiles, executable),
    which already carry the rest of what a command line is.
    *Proof:* the ratchet is red on the count of the day before its seed was
    lowered; putting one literal back turns it red.

## B. Dispatch: the same flow, from three seats, chosen by what is left

1. **`dispatch-the-work` chooses by fuel.** The dispatch step receives the
   remaining of every candidate (A.1) and the pact (A.4) and writes
   `first_engine`/`second_engine` with a `why` per choice; the ledger keeps the
   why on the run. A choice without a why is out of shape.
   *Where it landed:* `why_first` and `why_second`, required in the dispatch
   step's `answer_shape` and output schema of `dispatch-the-work`; the whys
   live in the step's recorded output. Not yet: the remaining and the pact
   are not injected into the dispatch prompt.
   *Proof:* a dispatch answer without the whys fails the shape and the run
   stops on that step; with them the chain runs to its verdict.
2. **Three seats, one ledger.** The same flow is started from this terminal
   (a Claude Code session under the Sailor hooks), from a Sailor pane in the
   window, and from a bare `sailor flow run` in a shell; the three runs are
   three rows in `runs`, each with its `started_by` naming the seat, and the
   window's Now shows all three while they run.
   *Where it landed:* `started_by` is written by the system: «sailor flow,
   in a Sailor terminal» when the pane's mark (`SAILOR_TERMINAL`) is in the
   environment, «sailor flow, in a shell» otherwise, and the window's own
   origin from the button or the beat. An agent's session under the hooks is
   not told apart from the shell yet: the hooks leave no mark in the
   agent's environment.
   *Proof:* a test on `started_by` for the two command-line values; the
   three runs from the three seats are Theo's gesture (see the end).
3. **Two engines, two trees.** Each engine of the parallel front works in its
   own git worktree of the project, named after the run and the step, and the
   verify step reads both trees. No two engines share a tree.
   *Where it landed:* a step declares `"tree": "own"` and works in a worktree
   cut under `<repo>-worktrees/<run>/<step>`, detached so no branch is left
   behind; a retried step finds the one its first attempt left. The two engine
   steps of the shipped dispatch declare it. A step that asks for a tree and
   also names a `workdir` is refused, and so is one whose run cannot say which
   project it is in: the shared tree is never the quiet fallback. The
   executor stopped handing the root to a step that asked for its own, or the
   `workdir` it never wrote would read as a contradiction.
   *Proof:* two steps of one run stand in two different trees, both checkouts
   git lists, both named after the run; the same step run twice stands in the
   same one; with the tree unwired both stand in the directory the process
   started in.
4. **The judge is blind by construction.** The verify step runs in a fresh
   session (never `--continue`), receives only the mandate and the two
   answers, and its verdict is a closed word. The descriptor's `fork_session`
   is never used for it.
   *Where it landed:* a step declares `"blind": true`, and a blind step opens
   no session and continues none, whatever it asks; `sailor flow check`
   refuses a flow that declares a step blind and also asks it to resume or
   fork, because the two say opposite things about the same step. **The word
   is the flow's, not the code's:** Theo, on 03/09, on a first version that
   made `kind: judgement` mean blindness — *shouldn't the judge be a node a
   user creates rather than something hardcoded?* A kind of work is data for
   the strengths table; what a step may see is the author's declaration, and
   nothing in Rust reads a step as a judge.
   *Proof:* with a session left by an earlier step and an engine that can
   resume, the same step resumes when it says nothing and starts from scratch
   when it says `blind`; the check names a step that declares both.
5. **The mechanical work goes local, and is judged by a check.** The ollama
   descriptor gains an `ask` block (`ollama run <model>`, prompt on stdin,
   `usage` from `--verbose`), and a flow `sweep-the-tree` takes a mechanical
   mandate (an English sweep, a rename, a comment cut), runs it on the local
   coder model file by file in a worktree, and closes green only on
   `cargo test` and the ratchets, never on the model's word. Claude is not a
   candidate of that flow.
   *Where it landed:* the ollama descriptor's `ask` and `usage` blocks
   (measured with qwen2.5-coder:1.5b, pulled on 03/09; the 7b is the
   upgrade); the shipped flow `sweep-the-tree`: one file named in the
   trigger's `where`, read as committed through `git`, rewritten by the local
   runner alone (`kind: mechanical`, `data: private`), and handed over whole
   as a proposal, with `cargo test` and the ratchets as the person's closing
   gesture. Not yet: file by file, the worktree, and the check as a step: a
   proposal is data until C.1's `apply_patch` exists, so nothing writes.
   *Proof:* the shipped flow names the local runner as its only engine and
   ends in a handover; run without spending, the file read reaches the model
   and the proposal reaches the handover whole while the run waits for a
   person. *Measured on 03/09:* two real runs on `crates/models/src/fuel.rs`
   from this terminal: rows on `ollama`, `work_kind: mechanical`, counts
   read from stderr (2313 in, 1934 and 41 out), zero Claude calls; the first
   run died on a ledger column the schema version had not carried, fixed
   with a test. The 1.5b model answered a placeholder instead of the file:
   the sweep needs the 7b, which is Theo's pull.

## C. Sailor develops Sailor, and this terminal watches

1. **The three rings hold.** Ring 1: no self-care step has write powers; a
   proposal is a diff as data in an `answer_shape`. Ring 2: an `apply_patch`
   action in Rust, with a compiled deny list (the authorisation file, the
   apply module, its test) that no data can widen, and an allow file in
   `$SAILOR_HOME/autocura.json` outside the tree. Ring 3: the step that hands
   the patch is `HandToHuman`, taken and closed from the window.
   *Where it landed:* `crates/actions/src/apply.rs` registers `apply_patch`,
   whose species is `HandToHuman`. It reads the paths a unified diff names,
   asks the compiled wall first — the assent file, the applying module, the
   canary that defends them — then the assent in `$SAILOR_HOME/autocura.json`,
   and only then hands the patch to `git apply`, `--check` first. No assent
   file is no permission, never «anything goes». Ring 1 and ring 3 were
   already standing: the sweep flow's proposal is data in an `answer_shape`,
   and `handed_to_agent` is the step a person closes. Still to do: the
   fingerprint of the assent file written into the ledger at every run,
   sentinel (b) of the design note.
   *Proof:* the canary in `crates/sailor/tests/the_wall_a_patch_cannot_widen.rs`
   asks, under an assent naming everything, to repair the assent file and the
   applying module: both refused, while an ordinary file under the same assent
   goes through and is read back changed. Taking the assent file out of the
   wall turns the first arm green, which was run.
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
   *Where it landed:* the shipped flow `write-down-what-broke`. It asks the
   ledger how the named flow's last closed run went, reads the faults already
   open, and has an engine write the line — what happened, how it showed, and
   the check that would have caught it, which the register refuses a fault
   without. The engine also answers whether the register already holds this
   defect, and a condition on that answer decides whether the recording step
   opens: the engine that writes never decides whether its line lands.
   *Proof:* with the engine answering «already written» the recording step
   never opens and the run still closes; with the opposite answer the same
   graph writes, and the register gives it a number.
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
   *Where it landed:* the shell's `sailor_event` channel (`events.rs`),
   fed by runs, the beat, the build and a terminal that closed; the bar has
   no timer left: it asks once and asks again at every fact on that
   channel, and a refused ask is still shown red with its reason. The
   older channels stay for the views that listen to them; Now is not yet
   a view of the same events.
   *Proof:* a test that the bar subscribes to `sailor_event` and to no
   other channel, and asks again when a fact arrives; the mute state on a
   refused ask stays.
4. **Theo is asked only for choices.** Every question the system puts to a
   person is a `HandToHuman` step with closed options and the facts each
   option rests on; free-text questions do not exist in flows. The bar
   counts the choices waiting; Now lists them first; the same list is
   printed by `sailor step list` for a terminal. What is not a choice is done
   and reported.
   *Where it landed:* `options` on a handed step (`label`, `facts`), printed
   in the brief; `flow check` names the handed steps without a closed
   choice and refuses the flow; the shipped sweep's handover offers
   «applied» and «refused». Not yet: the bar's count, Now's list, and a
   `sailor step list` (the command does not exist).
   *Proof:* a handed step without options is refused by `flow check`.
5. **The six gaps of the judges are closed or deprecated with a reason.**
   Bar carries three facts only; Sailor › models is a catalogue with «in use»;
   can do shows surface and powers; keeps shows growth and identities; the
   band shows state and cost per flow; the ledger keeps a run's root.
   *Proof:* the judges' list in `la-finestra-che-mostra-se-stessa.md` marked
   one by one.

## F. Clearing another tool's grafts off the machine

**Taken out of this repository on 2026-09-04, by Theo's decision.** It was a
section named after somebody else's product, describing its removal — which is
the one thing `AGENTS.md` has kept out of here since the founding charter:
framing this work as a reaction to another product. The measurements stay; the
positioning went to the notes that have no remote.

What the letter of it asked for is still worth building, and in a shape that
does not name anything: **a tool declares where it puts its grafts and its
data, and Sailor knows how to clear them without knowing the tool.** That is
the descriptor, the same door an engine comes in through, and it works for any
tool instead of one. Nobody has built it.

## G. A public repository, and what a reader of the code found

The repository is public since 02/09/2026. Theo, the same night: *let us
grind, and remove what should not be public: local or reserved flows live on
the machine, or in the private repository of the user's own that Sailor
connects them to.* An external agent read the code that evening (about 65k
lines of Rust, 16 crates, 980 tests) and left a judgement; what it found
enters here as claims, not as praise.

1. **Nothing reserved is tracked.** No flow of a person's own, no profile
   home, no credential, no path into a home directory is in the tree; the
   flows that ship are the system ones and this project's; a test lists what
   `git ls-files` may contain under `flows/` and refuses the rest.
   *Where it landed:* `crates/sailor/tests/nothing_reserved_is_tracked.rs`
   lists the five flows `git ls-files` may hold and the file names that are
   a person's (`.credentials.json`, `auth.json`, `profili.json`,
   `cooldowns.json`, `budgets.json`, `.env`).
   *Proof:* adding a flow file under a home-only name to the tree turns the
   test red.
2. **A person's flows sync to a repository of their own.** `sailor flows
   publish` (the name is the mandate's to choose) initialises or reuses a
   git repository the person names in `sailor.json` or in the home, private
   by default, pushes the home's flows there, and **refuses to publish a flow
   that carries a secret**: a value that looks like a key or a token, or an
   `env` block with a literal. The refusal names the step and the key.
   *Where it landed:* `sailor flow publish [remote]`, in
   `crates/sailor/src/publish_cmd.rs`. It takes the flows of the source that
   is the person's own, refuses on the first secret before touching git, then
   initialises the repository if there is none and commits what changed. The
   remote is named once and remembered by git as `origin`, so a later
   publication needs no argument; a second, different remote is refused
   rather than silently splitting the history. Nothing here talks to a forge:
   creating the repository, private, stays the person's gesture.
   *Proof:* a flow with `"OPENROUTER_API_KEY": "sk-…"` is refused and nothing
   is even initialised; the same flow with `{"$env": "OPENROUTER_API_KEY"}` is
   published. Reverting the token test to a match anchored at the start of the
   value, or letting the walk judge a string twice, turns the tests red — both
   were real defects the tests caught.
3. **Clippy gates what is dangerous and counts the rest.** `clippy::correctness`
   and `clippy::suspicious` are errors in CI; the other groups are a counted
   ratchet, as the comments are. The reviewer counted 173 warnings never
   gated.
   *Where it landed:* the `gate` job of `the-battery.yml` runs clippy with
   the two groups as errors and stops the battery; the tree passed it on
   03/09 after two fixes (an orphan doc comment, a comparison with zero
   that was always true). The other groups stay the counted debt of the
   `style` job.
   *Proof:* a deliberate `suspicious` lint in a scratch commit fails CI.
4. **A long process is started only with the supervisor's token.** Fault 4's
   cure is typological: the function that spawns a long-lived process takes a
   value only the supervisor emits, so a second road does not compile.
   *Proof:* a test that tries to spawn without the token does not compile,
   asserted with a `compile_fail` doctest.
5. **`Some(Null)` and `None` are two records.** The event register wraps the
   value so the two serialise differently; the guard in `step close` stays,
   but the format no longer depends on it (fault 33, half open).
   *Proof:* a round trip through the register keeps the two apart.
6. **The two files out of scale are split by responsibility.** `actions/src/lib.rs`
   (6 443 lines) and `sailor/src/flow_cmd.rs` (6 253 lines, 71 functions)
   become modules: at least `cost`, `cap` and `schedule` out of `flow_cmd`.
   *Where it landed:* `files_do_not_grow_out_of_scale.rs`, seeded with the
   seven files over 2 000 lines on 03/09 (`actions/src/lib.rs` 7 206,
   `flow_cmd.rs` 6 799, `ledger/src/lib.rs`, `ledger/src/tests.rs`,
   `session_cmd.rs`, `flow/src/executor.rs`, `desktop/src/App.tsx`); the
   split itself is still to do.
   *Proof:* no file in the workspace over 2 000 lines, as a ratchet seeded
   with today's count.
7. **Comments do not talk more than the code.** Theo, 03/09: *unacceptable*.
   Measured that night, comment lines over code lines per crate: release
   0.52, registry 0.41, actions 0.39, desktop 0.34, toolbox 0.30, models 0.29,
   terminal 0.29, inventory 0.28, sailor 0.27, catalogue 0.27, sessions 0.25,
   trigger 0.25, flow 0.22, faults 0.21, ui 0.19, workspace 0.19, profiles
   0.19, ledger 0.17, relay 0.16; the whole tree 0.28. A ratchet per crate is
   seeded with those numbers and may only fall; the pruning is a mechanical
   mandate for `sweep-the-tree` (B.5): a comment stays only where the code
   alone misleads, at most two lines; the why that still holds moves to the
   faults document or the commit that introduced it; chronicle is removed.
   *Where it landed:* `COMMENT_PERMILLE_TODAY` in
   `comments_do_not_crowd_out_the_code.rs`, one exact seed per crate
   measured on 03/09 with the same comment reader as the other three
   ratchets (release 520‰, registry 409, actions 371, supervisor 316,
   toolbox 297, terminal 287, inventory 278, models 276, desktop 273,
   catalogue 268, sailor 264, sessions 253, trigger 248, flow 221, faults
   212, ui 192, workspace 191, profiles 183, ledger 170, relay 155); a seed
   above the tree is red too.
   *Proof:* the ratchet is red on any crate whose ratio rose; a crate under
   0.10 is the target the mandate reports against.
8. **The front door is in English and says what it solves.** The README opens
   with what a person gets in five minutes, and the installation is one line
   (`cargo install` or a release binary); the documents that explain why the
   project exists get an English summary at their top.
   *Where it landed:* the README opens with «Five minutes»: install with
   `cargo install --path crates/sailor`, then the five commands that show
   the engines on this machine, try their command lines without spending,
   and run a mechanical job on the local model. The commands table names
   `publish`, `endpoint` and `models`; the language paragraph says the
   Italian left is a measured debt and not a plan. Still to do: the release
   binary as the other one-line install, and the English summary at the top
   of the documents that are still in Italian.
   *Proof:* Theo's gesture: a stranger reads the README and installs.

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

Theo, 03/09 at night: *priority to the orchestration of the accounts and to
the window; tomorrow I start working in Sailor's terminals.* So the order is:
first what a person needs to work in the terminals tomorrow (the engines
screen with sign-in and install, E.2, D.1), then A, then B, then E, then C,
then D, then G, then F. A.6 is asked of Theo at the start and does not block
A.1–A.5. Inside a group the order is free.

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
