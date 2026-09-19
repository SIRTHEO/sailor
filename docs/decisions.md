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

**Status:** proposed — recorded among the recommendations, not yet decided

**Context.** Complex agent systems frequently introduce auxiliary model calls—such as routing steps, summarizers between stages, context compressors, or automated LLM judges—under the premise of optimizing execution. In practice, flow costs and latency are dominated by turn counts rather than context token reductions. Auxiliary turns increase total cost, latency, and system opacity without offsetting benefits.

**Decision.** Do not introduce intermediate model calls (such as model routers, context summarizers, or step-evaluating LLMs) to optimize or judge other model calls. Verification must be performed using deterministic, executable code checks wherever an acceptance criterion is testable. An engine call is permitted only to resolve remaining ambiguities after deterministic checks have run.

**Consequences.** Auxiliary LLM middleware is strictly prohibited. Intermediate state transitions remain transparent and uncompressed. Operational efficiency is achieved through session reuse and skipping already successful steps rather than adding management turns.

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
