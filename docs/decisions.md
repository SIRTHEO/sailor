# Architecture decisions

This document records the architectural decision records (ADRs) and permanent constraints that bind all contributors to Sailor. The historical working record, including exploratory notes, dates, and machine-specific measurements, is maintained separately in the project's internal notes.

## ADR-001: Maintain independence from the model

**Status:** accepted

**Context.** Sailor must interact with any command-line tool or external engine, including tools that do not yet exist. Coupling the system to a single provider or engine breaks compatibility across different runtime environments. Workarounds that only function on one specific engine risk fragmenting the core execution logic.

**Decision.** Core execution logic must remain completely independent of any specific model or engine. Any engine-specific feature must be declared explicitly as a capability in that tool's descriptor. An engine lacking a capability must continue functioning via general fallbacks, even if doing so incurs higher token or execution cost.

**Consequences.** No engine-specific special cases are permitted in core execution code. Running flows on less capable engines increases operational overhead. The tool ecosystem remains open to new and heterogeneous CLI engines without engine-specific core modifications.

## ADR-002: Preserve clarity for whoever is looking

**Status:** accepted

**Context.** Sailor exists so that a person can inspect, understand, and control what their tools do. An optimization that makes intermediate state transitions or step interactions opaque undermines user trust and system inspectability. User interfaces and execution traces must accurately reflect operational reality rather than concealing it behind abstractions.

**Decision.** Never introduce optimizations or architectural shortcuts that make data passing between steps opaque. All step inputs, outputs, and intermediate states must remain directly visible and inspectable. Interfaces must clearly display what is occurring during execution rather than hiding activity behind opaque summaries.

**Consequences.** All step transitions remain inspectable and fully recorded in the store. Contributors cannot use hidden channels or private state shortcuts to save compute or tokens. Complete transparency guarantees auditability and reproducibility for every run.

## ADR-003: The screen is the judge

**Status:** accepted

**Context.** Static types and automated unit tests can both pass while the visual or terminal presentation remains broken or unreadable. Display expectations and layout rules that exist only as verbal guidelines cannot be verified reliably. Visual behavior requires an objective, inspectable standard of correctness.

**Decision.** Any project rule governing presentation or user interface behavior must be verifiable by inspecting rendered visual output. A rule or requirement that cannot be verified by looking at an image or rendered screen is treated as an unsubstantiated opinion rather than an accepted constraint.

**Consequences.** Visual behavior must be backed by inspectable screen output or visual verification checks. Subjective aesthetic preferences without observable rendering verifications cannot be enforced as project rules. Visual defects cannot hide behind passing unit tests.

## ADR-004: Whoever creates does not judge

**Status:** accepted

**Context.** An automated step or engine that evaluates its own generated output operates with its prior context and biases already loaded. Self-evaluation routinely fails to detect its own errors and leads to false positive approvals. Independent evaluation is necessary to maintain software quality and prevent regressions.

**Decision.** The verdict on any piece of work must be rendered by an entity that did not produce it. An engine, step, or agent is forbidden by default from judging, verifying, or approving its own output. The one exception is a step that declares `"same_holder_ok": true` (ADR-010).

**Consequences.** Verification steps require separate execution contexts or independent checks. Flows must be structured with explicit, independent validation boundaries. Self-approval is prohibited in every flow unless a step declares the ADR-010 exception.

## ADR-005: A test counts only if it could have come out differently

**Status:** accepted

**Context.** Automated tests that cannot fail under defective conditions provide illusory confidence. Tests written without verifying their failure modes often test tautologies or miss real execution paths entirely. A suite of constantly green tests may silently allow breaking regressions into the codebase.

**Decision.** When writing or modifying a test, contributors must intentionally break the underlying implementation and verify that the test fails (goes red) before accepting it. Any test that has not been observed to fail against broken code is invalid and must not be committed.

**Consequences.** Writing tests requires an active negative verification step during development. Test maintenance takes more time and discipline. The test suite maintains high diagnostic validity, preventing vacuous tests from entering the repository.

## ADR-006: Write in code only what touches the world

**Status:** accepted

**Context.** Embedding application logic and procedural workflows directly in compiled binaries makes them rigid and requires recompilation for every procedural change. Conversely, allowing uncompiled scripts to execute uncontrolled low-level system operations undermines safety and authority boundaries. A clean separation between the execution engine and workflow logic is required.

**Decision.** Compiled code is reserved strictly for components that directly touch the external world: the execution engine, the record store, and the authorization gate. All higher-level logic, procedural sequences, and workflows must be expressed as declarative flows that can be modified without recompiling the binary.

**Consequences.** Contributors must not implement workflow orchestration or business logic in compiled Rust code. The core binary remains small, robust, and focused on core capabilities. A project's or a user's own flows change without recompiling; the flows the product ships are embedded in the binary (ADR-011) and change with a rebuild.

## ADR-007: Use English everywhere

**Status:** accepted
*Superseded note:* Supersedes "The language: identifiers in English, all the rest in Italian" and "The language is chosen by who reads: English what a stranger sees, Italian what whoever works here sees".

**Context.** The repository previously suffered from fragmented language rules dividing internal notes, comments, and public interfaces. Because the repository is public and permanently world-readable, maintaining separate inside and outside languages creates ongoing confusion and defect-prone translation boundaries. A single universal language standard is required across all project artifacts.

**Decision.** All identifiers, code comments, documentation, commit messages, user-visible output, and test messages must be written in English. No distinction is made between internal and external communication. Flow IDs, step IDs, and `.flow.json` filenames remain preserved as immutable store data.

**Consequences.** Contributors must write all contributions in English and translate or prune existing non-English comments when modifying files. Comment verbosity must comply with line limits. Enforced by `cargo test -p sailor --test comments_do_not_crowd_out_the_code` and `cargo test -p sailor --test identifiers_are_in_english`.

## ADR-008: Flow and step identifiers are data and are not renamed

**Status:** accepted

**Context.** Flows and steps are identified by keys that are persisted directly in the execution store. Renaming an existing flow or step identifier breaks historical continuity, causing previously recorded runs to appear as unknown steps and breaking resumption after crashes. Furthermore, system flows embedded in the binary can be overridden by user flows matching their names.

**Decision.** Existing flow IDs, step IDs, and `.flow.json` filenames are immutable store data and must never be renamed for stylistic consistency. The check `identifiers_are_in_english` applies strictly to code identifiers and must never inspect flow IDs, step IDs, or flow filenames.

**Consequences.** Historical runs in the store remain valid and resumable. Local user-defined flows continue to override built-in system flows reliably. Asymmetries in naming across legacy flow definitions are preserved as data invariants.

## ADR-009: References are resolved in one place only

**Status:** accepted

**Context.** Allowing individual actions to resolve their own input references creates duplicate resolution logic and divergent execution semantics across the graph. If references are resolved before evaluating step conditions, a skipped step may fail unexpectedly when resolving missing upstream dependencies. Step input records in the store must also have clear, unambiguous semantics.

**Decision.** Reference resolution occurs solely within `flow::step_input` as the single canonical point of reference processing. The execution sequence must strictly compose dependencies with `with`, resolve `workdir`, evaluate the `when` condition, and resolve references only if the step actually executes. `StepRecord::input` records the input as received when processing ceased: resolved if the step runs, and unresolved if skipped or aborted during resolution.

**Consequences.** Individual actions never resolve their own references or `workdir`. Skipped steps never fail on unproduced upstream outputs. The invariant is strictly enforced by the structural guard `crates/sailor/tests/references_are_resolved_in_one_place.rs`.

## ADR-010: Hand execution to live agents without conceding judgement

**Status:** accepted

**Context.** Launching isolated child processes for every step incurs high token and context-discovery overhead compared to an active terminal agent. However, allowing a live agent that executes a step to also evaluate its success violates the permanent constraint of independent judgement. The system requires an ergonomic mechanism to delegate execution without compromising verification integrity.

**Decision.** Flows may delegate execution to a live terminal agent via `handed_to_agent`, using `sailor step open` and `sailor step close`. Whoever closed a step is strictly forbidden from opening or closing any step that depends upon it, unless explicitly overridden per step with `"same_holder_ok": true`. Handoff timeouts are governed by `handoff_timeout_secs` relative to `started_at` without querying operating system process tables.

**Consequences.** Agent turns and redundant repository rediscovery are significantly reduced. Self-judgement is blocked by default at both step open and close boundaries. Steps with agent handoffs record `cost_micros` as empty (`None`), causing `Spend::calls_without_cost` to increment and marking total spend as an unverified floor.

## ADR-011: System flows live inside the binary

**Status:** accepted

**Context.** Shipping standard system flows as standalone files on disk exposes them to accidental deletion, modification, or version drift. Discrepancies between file versions across installations cause silent behavioral divergence across environments. Core operational flows require guaranteed availability.

**Decision.** System flows must be embedded directly into the compiled Sailor binary at compile time under `crates/flow/system/`. When a flow is invoked, a local flow defined in the user configuration or current project workspace with the same name takes precedence over the embedded system flow.

**Consequences.** The binary operates out of the box with zero external flow file dependencies. Users can customize or override any system flow by supplying an identically named local `.flow.json` file. Updating default system flows requires rebuilding the binary.

## ADR-012: Flows compose rather than merge

**Status:** accepted

**Context.** Monolithic workflows that combine multiple functional phases—such as research, dispatch, development, and inspection—into single multi-step flows are inflexible and cannot be reused independently. When a large composite flow fails, partially executing or reusing individual phases becomes impossible.

**Decision.** Workflows must remain decoupled as discrete, focused flows that compose by invoking each other via subflows rather than merging into monolithic definitions. Each distinct phase must remain usable as an independent flow.

**Consequences.** Flow definitions remain modular, testable, and reusable across different operational contexts. Orchestration relies on subflow execution mechanisms. Contributors must not create all-in-one flows when individual stages have standalone utility.

## ADR-013: Capabilities are data and absence is written down

**Status:** accepted

**Context.** External command-line tools offer disparate features, such as structured outputs, session branching, or quota introspection. Hardcoding capability lists in application code creates tight coupling and requires recompiling whenever a tool changes. Furthermore, omitting a capability from a descriptor leaves downstream systems unable to distinguish between an uninspected feature and a verified absence.

**Decision.** Tool capabilities are defined strictly as data in tool descriptors under `capabilities`, mapping each capability name to its declaration (the arguments and notes that express it) or to `false`, without any compiled vocabulary in code. A value of `false` explicitly denotes that the capability was looked for and is absent, whereas omitting a field indicates nobody looked. Absence of a capability is not a failure; steps requiring absent capabilities issue warnings via `sailor flow check` and fall back to generic prompt-based mechanisms.

**Consequences.** Adding or updating tool capabilities requires only editing JSON configuration in `~/.config/sailor/tools.d/` with no code recompilation. Shipped tool descriptors must declare all known capabilities explicitly. Tested via descriptor verification tests ensuring completeness of shipped tool descriptors.

## ADR-014: A total with an unknown inside is shown as a floor

**Status:** accepted

**Context.** Aggregating execution costs when one or more calls lack cost data produces misleading figures if displayed as a simple sum. Displaying a partial sum with an auxiliary note fails to communicate reality because users naturally focus on the headline number. Misrepresenting partial spend as a complete cost leads to erroneous budgeting decisions.

**Decision.** Any cost calculation containing even a single call with an unknown `cost_micros` must never be rendered as a definitive number. The display logic must query `Spend::reading()` and explicitly format the total as a floor (e.g., "at least X") alongside the count of unmeasured calls. If no measurements exist, the cost must be displayed as unknown rather than zero.

**Consequences.** Contributors must use `Spend::reading()` for cost rendering and must not format raw numeric sums independently. Cost displays unambiguously communicate that actual spend exceeds the known floor. Eliminates false precision across all cost-reporting CLI commands.

## ADR-015: A person's quota is not the cost of a run

**Status:** accepted

**Context.** Provider accounts track cumulative quota consumption across multiple concurrent sessions, CLI tools, and historical windows. Using account quota delta as a proxy for an individual flow run's cost conflates external user activity with the run's actual consumption. This misattribution corrupts execution metrics and cost analysis.

**Decision.** Provider account quota and flow run cost must remain strictly separated into distinct concepts and commands. Remaining account quota is measured via read-only capability checks in `sailor remaining` and must never be written into step records or combined with run expenditures in `flow cost`. Tool descriptors must declare support for quota inspection via `read_remaining_quota`.

**Consequences.** Steps never report account-level quota consumption as run cost. `sailor flow cost` reports solely direct execution costs from declared step invocations. Quota introspection remains an optional capability that falls back gracefully when absent.

## ADR-016: An engine that does not declare how it runs out does not stand in the middle of a chain

**Status:** accepted

**Context.** Fallback execution chains depend on reliably detecting when an engine has exhausted its quota or capacity. If an intermediate engine does not declare its exhaustion signature (`ask.unusable_when`), its exhaustion manifests as an ordinary failure, terminating the step and preventing subsequent engines from executing. Chains configured with undeclared fallbacks silently lose their fallback guarantees.

**Decision.** Any engine that does not define `ask.unusable_when` in its descriptor is prohibited from occupying an intermediate (fallback) position in an execution chain. Such engines may only be positioned at the terminal end of a chain or used standalone.

**Consequences.** Fallback chains guarantee that execution reaches downstream engines when upstream providers are exhausted. A flow that breaks this placement rule is named by the checks that interrogate it: `toolbox::Descriptor::cannot_be_a_fallback`, `every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted`, and `sailor flow check`.

## ADR-017: You do not add calls to save calls

**Status:** accepted

**Context.** Complex agent systems frequently introduce auxiliary model calls—such as routing steps, summarizers between stages, context compressors, or automated LLM judges—under the premise of optimizing execution. In practice, flow costs and latency are dominated by turn counts rather than context token reductions. Auxiliary turns increase total cost, latency, and system opacity without offsetting benefits. Stated as a ban on a mechanism, the rule also refuses a case its own argument does not reach: a choice among options declared in advance, resolved in a single forward pass on hardware the operator already runs, adds no turn to a run and nothing to its metered bill. Two of the three objections miss it; latency and opacity remain, and opacity is the one that has caused harm, because a choice nobody sees is a choice nobody can contest.

**Decision.** Do not introduce an intermediate model call that adds a turn to a run in order to optimize or judge another model call. Verification must be performed using deterministic, executable code checks wherever an acceptance criterion is testable. An engine call is permitted only to resolve remaining ambiguities after deterministic checks have run. A local choice among declared options is not such a call, and is permitted only under all of the following: it adds no turn and nothing to the metered bill; its options are data, declared before the decision and versioned together with the wording that presents them; it yields a probability and abstains below a declared threshold instead of choosing anyway; it records what it was asked, the options offered, the one chosen, its probability and the threshold, where the run's record is read; and it decides a fact that can be read off the material, never a judgement. Whether something is safe, finished, or worth doing stays with the engine turn or with a person.

**Consequences.** Auxiliary middleware that adds a turn remains prohibited. A local choice that cannot publish what it decided does not act: it abstains. Because the options and their wording are versioned, a rewording is a new decision rather than the same one answered differently. Intermediate state transitions remain transparent and uncompressed. Operational efficiency is achieved through session reuse and skipping already successful steps rather than adding management turns.

## ADR-018: Place spending ceilings on flows and halt runs before opening fronts

**Status:** accepted

**Context.** Flows executing without budget limits can incur unbounded provider costs during unattended runs. A step that discovers a budget breach mid-action has already incurred costs, leaving the boundary before opening a front as the only zero-cost halting point. In addition, reporting budget exhaustion as a run failure creates false failure alarms for scheduled flows designed to touch their limits.

**Decision.** A flow may declare an optional `spend_cap_micros` attribute specifying the maximum spending allowed for a run. The default value is `None`, representing no limit, which must not be treated as `Some(0)`. Before opening each front, the executor queries the store for cumulative spending; if the ceiling is reached, the run stops with the status `cap_reached` rather than `failed` and records which steps did not start. The ceiling must not depend on engine-specific native spending caps, which govern individual invocations rather than entire runs. Spending totals must consistently be presented as equivalent cost across all commands.

**Consequences.** Halting is bounded by the cost granularity of a single call rather than micro-level increments because checks occur only before opening fronts. Ceilings guarantee boundaries only over known declared costs; invocations of engines that do not report cost metrics are omitted from the total, and stopped runs report the count of uncosted calls. Flow spending ceilings cannot be calibrated on fewer than three runs with known non-zero costs; `sailor flow cap` refuses to suggest a ceiling below this threshold. When calibrated, the suggested ceiling equals the worst observed run cost plus the dearest observed call cost.

## ADR-019: Assemble and dry-run engine command lines during flow check

**Status:** accepted

**Context.** Command-line syntax errors and flag misconfigurations previously went undetected until execution time. Static analysis and `--help` flags fail to validate argument structures because engines short-circuit before parsing real parameters. Furthermore, engines can return identical non-zero exit codes for expected prompt omissions and actual flag syntax errors.

**Decision.** `sailor flow check` must assemble the command line a descriptor composes for every engine across all chains and execute it without the prompt question; a step that supplies its own `args` is not dry-run. Dry-run checks are enabled by default, but contributors may disable them using `--no-engines`, in which case unverified lines must remain unreported rather than marked healthy. An engine descriptor declares either `refuses_without_prompt`, the engine's exact refusal message, or `silent_without_prompt` for an engine that says nothing when asked no question. The validation logic in `judge_dry_run` must evaluate stdout and stderr text without inspecting or receiving process exit codes. Conditions in `unusable_when` must be evaluated before `refuses_without_prompt` to prevent misclassifying unavailable engines as malformed. Resolving engine names must remain strictly static, confining all process execution exclusively to check workflows.

**Consequences.** `sailor flow check` is no longer strictly static and spawns child processes locally, requiring an explicit execution timeout without relying on system-specific timeout commands. Malformed command lines and invalid flags are caught locally before flow execution without contacting providers or incurring spending. The dry run does not verify whether an engine has actually been invoked in historical runs, which remains independently tracked by the store. Dry-run outcomes are verified and categorized by `judge_dry_run` into healthy, broken, not tried, not assemblable, or unusable states.
## ADR-020: The product names no forge, no remote, no trunk and no account

**Status:** accepted

**Context.** ADR-001 and ADR-013 bind Sailor to know no engine by name: a tool introduces itself with a descriptor, and a capability nobody declared triggers nothing. Delivery never made the same promise. `sailor policy`, the command that reads the delivery policy from the trusted trunk, carried the trunk's name as a string literal, so a repository whose trunk is not called `main` received `no_policy_on_trunk` and refused its own policy with nothing saying why. The flows the product ships name a forge's program 30 times, a remote 62 times and a trunk 92 times inside their shell. The repair of the delivery loop turns those shell blocks into registered actions, and done without this rule first it would move the same names from a data file into the compiled binary, where changing them requires a rebuild rather than an edit.

**Decision.** Core code and shipped flows must not name a forge, a remote, a trunk or an account. Each is a fact about one repository, and each is declared by that repository: in a descriptor, in `.sailor/delivery-policy.json`, or in a `sailor.*` key of the repository's own git configuration, the way `sailor.forgeAs` and `sailor.pushAs` already are. What is not declared makes the product name what is missing and stop; it must never fall back to a value that happens to be right here. This is ADR-013's "absence is written down" applied to delivery instead of to engines.

**Consequences.** Adding a forge means writing a descriptor, not a branch in the code. A repository whose trunk is called something else works without a change to Sailor, and one that has declared nothing is told which declaration is missing rather than silently refused. The counts are held by `cargo test -p sailor --test no_forge_no_remote_no_trunk_is_named_in_the_code`, which measures the code and the shipped flows separately, on a clean `HEAD`; both seeds may only fall. The shipped flows carry the larger count, and it comes down as the delivery loop becomes registered actions.

## ADR-021: The product names no tool of the workbench

**Status:** accepted

**Context.** ADR-001 binds Sailor to know no engine by name, ADR-013 makes a capability data that nobody's absence triggers, and ADR-020 says a forge, a remote, a trunk and an account are facts a repository declares. All three answer the same question about a different half of the product, and a fourth half had no answer at all: the tools standing on the machine Sailor happens to run on. `crates/workspace/src/index_identity.rs` publishes `PIN_FILE = ".socraticode.json"`, `PIN_VARIABLE = "SOCRATICODE_PROJECT_ID"` and `BRANCH_AWARE_VARIABLE = "SOCRATICODE_BRANCH_AWARE"`, and `sailor worktree` and `retire_index` call into it: whoever downloads Sailor to orchestrate their own flows gets one particular code index compiled into the binary. `.socraticode.json` and `.socraticodeignore` are committed at the root of the public repository for the same reason. Nothing refused any of it, because every gate this repository has counts a quantity — comments, tests, names — and none of them asks whether a line belongs to the product or to the machine it was written on.

**Decision.** Core code must not name a tool of the workbench: an index, an editor, an MCP server, a plugin, a marketplace, a skill. Each is a fact about one machine, and each is declared in a descriptor — `crates/toolbox/descriptors/default.json` and `~/.config/sailor/tools.d/` are already where a tool introduces itself, with `detect`, `config` and its capabilities. A feature that needs a tool asks the descriptor for the capability it needs; a capability nobody declares triggers nothing, and the product names what is missing rather than assuming the tool that happens to stand here. A tool's own configuration files are the machine's, not the repository's, and are not committed.

**Consequences.** Adding an index means writing a descriptor, not a module. The same feature works for a person whose index is a different program, and for one who has none. Held by `cargo test -p sailor --test no_workbench_tool_is_named_in_the_code`, whose seed may only fall. The seed starts above zero: it counts what stands today so the rule lands at the number it holds, and comes down as each naming is moved into a descriptor.

## ADR-022: A source file holds code, not its own suite

**Status:** accepted

**Context.** Thirty files in this tree pass a thousand lines, and the owner named that as a violation of the practice the repository claims to hold itself to. Measured apart, the two complaints turn out to be one: `crates/sailor/src/session_cmd.rs` is 3.457 lines of which 2.545 are its test module, `crates/actions/src/cost.rs` is 2.136 of which 1.898 are, and once the suites are not counted the thirty become eleven. A hundred and sixty-three source files carry their suite written inside them; three do not, and write `#[cfg(test)] mod tests;` pointing at a file beside them, which is what Rust provides and what `crates/ledger`, `crates/toolbox` and `crates/sailor/src/flow_cmd` already do. The number had never been measured, so it had never met resistance.

**Decision.** A test module is declared, not written, in the file it tests: `#[cfg(test)] mod tests;` beside a `tests.rs`. Held by `cargo test -p sailor --test a_source_file_holds_code_not_a_suite`, whose seed starts at the 163 the tree holds and may only fall. **The ceiling on a file's length is not part of this decision**: `files_do_not_grow_out_of_scale` already holds it at a thousand lines and weighs product lines apart from judge lines, which is the better measure and was already there.

**Consequences.** The number cannot grow, so the debt is frozen the day this lands and is paid file by file, each payment lowering the seed in the commit that makes it. A suite moved out shortens the file it leaves, so the two ratchets fall together without either one naming the other.

**And why the ceiling was built twice.** `docs/gates.md` is what a worker, a reviewer and the integration all read, and it named six of the fifteen ratchets standing in the tree. Whoever reads the list therefore runs a shorter one than the trunk does and meets the rest in the report — and in this case built a ceiling that already existed, having read the list and found none in it. `every_declared_check_names_a_test_that_exists` kept the list from naming a test that is gone; `every_ratchet_is_named_in_the_gates` now keeps the tree from holding a ratchet the list never names. The list is complete in both directions, or one of the two goes red.

## ADR-023: A flow that takes something can give it back

**Status:** accepted

**Context.** A flow takes things that are not its own: a lease on a GPU, a lock, a temporary directory, a slot on a shared machine. Until this decision a step that broke as many times as it may made the run `Failed`, and nothing downstream of it started, so the step that would have given the thing back never ran. `when` reads input values and cannot see a break; `compensate` undoes one step's own effect before it is retried and is not a place for a flow to act; and the only escape was `accept: ["failed", "timed_out"]` on a check, which hides the break from the run it happened in. A lease with a time to live comes back in the end, but until it does the machine that granted it is without its service, and a lock without one stays held until a person notices.

**Decision.** A step may declare `even_after_a_break: true`. It starts once every one of its dependencies has settled — closed, or not closed because of a break: broken past its attempts, or never opened because the run broke first — instead of only when all have closed. It is handed its dependencies by id, the outputs of those that closed, and under `broken` the ids of those that did not; a dependency skipped by its condition is in neither, and `broken` is there and empty when nothing broke. Once anything in a run has broken, only such steps are opened, and the run waits for them: a run with one still to come is parked on whatever it waits for, not closed. **Giving back never turns a failed run green**: the run ends `Failed` with the steps that broke, and a step that gives back and breaks itself is counted among them. `sailor flow check` refuses the declaration on a step with no dependencies, and on a step with a dependency called `broken`, whose output that list would hide.

**Consequences.** A flow names what it gives back in its own graph, as data, and the executor names no tool, forge or machine to do it. The step runs once per run: it is settled like any other once it closes, so a resume finds it done. A halt by hand, a wall or a spend cap still holds the front before it opens, so a step that gives back is not run past one of them; ADR-024 takes that case up.

## ADR-024: Heavy work takes the machine's turn, and a stop still gives back

**Status:** accepted

**Context.** Every build sized its compilers from the memory free at the instant it started, and nothing stood between two processes that did so at once: a release, a ratchet and two integrations compiling together pushed the machine into swap until it stopped answering. Separately, a run that stopped short on its wall, its turns, by hand, on a passed check or at its spend cap held the front before the step that gives back its tree could open (ADR-023 left that case open), and every such run left a worktree and its build on the disk: about a hundred gigabytes before anyone looked.

**Decision.** A step may declare `weight: heavy`. Before its action starts it waits for the machine's turn: an exclusive `flock` the kernel ends with its process, in the order the waiters asked, each waiter a ticket that the next reader drops once its process is gone. The holder hands a token to what it starts, as the `SAILOR_MACHINE_TURN` variable and to the flows it calls, and whatever carries it runs inside the turn instead of queueing behind it. `sailor release` and `sailor ratchet` take the turn themselves, and `sailor machine turn -- <command>` takes it for any other command. A run stopping short of its end first opens the steps that give back which it still owes: never started, with everything before them settled, where the work the stop leaves unstarted counts as fallen, as a break would count it; the stop is then met again and recorded as before.

**Consequences.** No clock and no sweeper decides: the turn ends with the process that held it, and what a run took is given back on the way out of the run itself. A run parked on a person keeps what it took, since whoever resumes it may still need it. A command started outside Sailor that does not ask for the turn is not held by it: AGENTS.md asks for `sailor machine turn` in front of every build and every suite.
