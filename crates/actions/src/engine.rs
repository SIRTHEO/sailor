//! The engine action: a step asks an engine, or a chain of them, and what it
//! answers becomes the step's output or breaks it.

use crate::answer::{
    asked_again, check_tolerance, how_it_exited, shape_was_asked_for, shaped_answer, tolerates,
    what_it_said, ENGINE_FAILURES,
};
use crate::candidates::{strengths_path, Candidate, Refused};
use crate::cost::{
    current_price_list, now_secs, record_the_call, recording_for, Chain, Recording, Spent,
};
use crate::equipment::current_equipment_for;
use crate::process::{
    invoke_external_engine_watched_until, sink_for_step, EngineInvocation, EngineResult, LiveSink,
    Pipe, StepSinks,
};
use crate::recipe::{PromptVia, ToolResolver};
use crate::session::{session_plan, this_step_share, SessionPlan};
use crate::spec::{EngineSpec, A_TREE_OF_ITS_OWN, TREE};
use crate::{budget, cooldown, reserve, Reading};
use flow::{Action, ActionError, ActionOutcome, Ran, SharedState, StepSpecies, ValueSchema};
use ledger::{EngineIdentity, Ledger};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// ── the two actions registrable in a flow::ActionRegistry ───────────────

#[derive(Debug, Serialize)]
struct EngineOutcomeJson {
    status: &'static str,
    stdout: String,
    stderr: String,
}

/// Invokes an external engine, reading its recipe from the step's typed input;
/// it knows nothing of which engine, or which queue, calls it. The recipe may
/// carry references (`reference`): that is how one engine's brief becomes the
/// next engine's input.
///
/// **A FAILURE BREAKS THE STEP.** A nonzero exit, a timeout, a binary that will
/// not start: each closes the step as broken, carrying the why and the last
/// lines the engine said, and the steps depending on it do not start — else
/// they start on emptiness with a real call already spent. **The opposite can
/// still be said**: `"accept": ["exit_error"]` puts that outcome back among the
/// data with `status` saying so, but it must be written, and it holds for the
/// named outcome only.
pub struct ExternalEngineAction {
    pub(crate) tools: Option<Arc<dyn ToolResolver>>,
    pub(crate) watcher: Option<Arc<dyn StepSinks>>,
    pub(crate) ledger: Option<Ledger>,
    /// Where the engines set aside for a spent quota are listed; `None` when
    /// the machine has no home to keep the list in, and then nobody is aside.
    pub(crate) cooldowns: Option<PathBuf>,
    /// Where the person's spend caps per engine live; `None` means no cap.
    pub(crate) budgets: Option<PathBuf>,
    /// The person's strengths table, or `None` for the shipped one.
    pub(crate) strengths: Option<PathBuf>,
}

impl Default for ExternalEngineAction {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalEngineAction {
    /// With no resolver: a step asking for a tool by id gets an error saying
    /// how it is repaired, rather than a guessed binary.
    pub fn new() -> Self {
        Self {
            tools: None,
            watcher: None,
            ledger: None,
            cooldowns: cooldown::default_path(),
            budgets: budget::default_path(),
            strengths: strengths_path(),
        }
    }

    /// With a resolver: `"tool": "codex"` becomes the path that stands for
    /// `codex` on this machine.
    pub fn resolving_with(resolver: impl ToolResolver + 'static) -> Self {
        Self {
            tools: Some(Arc::new(resolver)),
            watcher: None,
            ledger: None,
            cooldowns: cooldown::default_path(),
            budgets: budget::default_path(),
            strengths: strengths_path(),
        }
    }

    /// With the person's spend caps read from `path`: a test hands a scratch
    /// file, so no test reads the machine's own caps.
    pub fn budgeted_by(mut self, path: Option<PathBuf>) -> Self {
        self.budgets = path;
        self
    }

    /// With the strengths table read from `path` instead of the shipped one.
    pub fn strong_by(mut self, path: Option<PathBuf>) -> Self {
        self.strengths = path;
        self
    }

    /// With the list of engines set aside kept at `path`: a test hands a
    /// scratch file, so no test writes the machine's own list.
    pub fn cooling_down_in(mut self, path: Option<PathBuf>) -> Self {
        self.cooldowns = path;
        self
    }

    /// With someone watching: the engine's text reaches them as it comes out,
    /// marked with the step producing it. `None` means nobody watches, and it
    /// is the value the action is born with — whoever registers it decides,
    /// not this crate.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }

    /// With a store to record what each call cost.
    ///
    /// **WHY THE STORE ARRIVES BY CONSTRUCTION AND THE RUN DOES NOT.** Whoever
    /// builds the action registry already holds the store open — the same road
    /// `store::register_store` has always taken — but has no run yet: the
    /// `run_id` is born later, as the run is about to start, so it comes from
    /// the shared state (`flow::CURRENT_RUN`) the executor fills step by step.
    /// `None`, the value the action is born with, records nothing.
    pub fn recording_to(mut self, ledger: Option<Ledger>) -> Self {
        self.ledger = ledger;
        self
    }
}

/// The tree one step works in, taken down when that step ends however it ends.
///
/// Closed on drop and not by a line at the bottom: an engine step leaves by a
/// dozen paths, and a tree closed on one of them is a tree left open on the
/// other eleven. See fault 89.
struct OwnTree {
    repo: PathBuf,
    at: PathBuf,
    live: Option<Arc<dyn LiveSink>>,
    /// The register the tree was written into, so the same binding that takes
    /// it down takes it off the page. See fault 97.
    register: Ledger,
}

impl OwnTree {
    /// A kept tree is named where the tree was announced, or the run ends with
    /// disk nobody accounted for.
    fn say(&self, sentence: String) {
        match self.live.as_deref() {
            Some(live) => live.chunk(Pipe::Stderr, format!("[sailor] {sentence}\n").as_bytes()),
            None => eprintln!("{sentence}"),
        }
    }
}

impl Drop for OwnTree {
    fn drop(&mut self) {
        let at = self.at.to_string_lossy().into_owned();
        match workspace::close_tree(&self.repo, &self.at, &self.register) {
            workspace::Closing::TakenDown => {}
            workspace::Closing::GitRefused(said) => self.say(catalogue::say(
                "engine.tree_kept_over_work",
                &[("tree", &at), ("said", said.trim())],
            )),
            workspace::Closing::HoldsACommitNobodyElseHas(commit) => self.say(catalogue::say(
                "engine.tree_kept_over_a_commit",
                &[("tree", &at), ("commit", &commit)],
            )),
        }
    }
}

/// The worktree this step works in, cut now if it is not there yet.
///
/// Refused rather than run in the tree everybody shares: a step that asked to
/// be alone and silently got the shared tree writes over another engine's work.
fn tree_of_its_own(
    spec: &EngineSpec,
    shared: &SharedState,
    live: Option<Arc<dyn LiveSink>>,
    register: Option<Ledger>,
) -> Result<Option<OwnTree>, ActionError> {
    let Some(asked) = spec.tree.as_deref() else {
        return Ok(None);
    };
    if asked != A_TREE_OF_ITS_OWN {
        return Err(ActionError::new(
            "invalid_input",
            format!("`{TREE}` knows only «{A_TREE_OF_ITS_OWN}», not «{asked}»"),
        ));
    }
    if spec.workdir.is_some() {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step asks for a `{TREE}` of its own and also names a `workdir`: \
                 the two say different places, and one of them would be ignored"
            ),
        ));
    }
    let said = |key: &str| shared.get(key).and_then(Value::as_str).map(str::to_owned);
    let (Some(root), Some(run), Some(step)) = (
        said(flow::WORKSPACE_ROOT),
        said(flow::CURRENT_RUN),
        said(flow::CURRENT_STEP),
    ) else {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step asks for a `{TREE}` of its own, and this run says neither which \
                 project nor which run and step it is: there is no name to cut it under"
            ),
        ));
    };
    let Some(register) = register else {
        return Err(ActionError::new(
            "tree_not_cut",
            format!(
                "the step asks for a `{TREE}` of its own and this run has no store: a tree \
                 nobody wrote down is disk nobody would ever come back for"
            ),
        ));
    };
    let repo = PathBuf::from(&root);
    workspace::tree_for(&repo, &run, &step, &register)
        .map(|at| {
            Some(OwnTree {
                repo,
                at,
                live,
                register,
            })
        })
        .map_err(|why| ActionError::new("tree_not_cut", why))
}

/// What asking **one** engine of the chain came to.
enum Asked {
    /// This engine answered: that is the step's answer, however it went. Nobody
    /// after it is tried.
    Answered(ActionOutcome),
    /// This engine declared it cannot work — in the words its own descriptor
    /// declares, not in a reading of ours. The next one is tried.
    CannotWork(String),
}

/// Why each one was set aside, in a single line.
fn each_one_why(reasons: &[String]) -> String {
    if reasons.is_empty() {
        return "Nessun motivo registrato.".to_owned();
    }
    reasons.join(" · ")
}

/// What happens to a step when the engine said it **cannot work**.
///
/// **IT LIVES IN ONE COPY, AND THAT IS NOT FUSSINESS.** The *rule* — which
/// words count — was already in one place (`mentions_any`); the **consequence**
/// was not, and its two copies, one per branch, diverged from birth. Fault 10
/// back in through the side door, on the very code repairing the fallback.
///
/// The difference this function keeps is what matters to a reader: **with a
/// chain behind it** the work passes on and there is no error to give yet;
/// **on its own** there is no next, but the diagnosis stays — «exhausted» says
/// wait or change profile, «exited in error» sends a reader hunting for a
/// fault that is not there.
fn engine_cannot_work(
    named: &str,
    solo: bool,
    stdout: &str,
    stderr: &str,
) -> Result<Asked, ActionError> {
    if !solo {
        return Ok(Asked::CannotWork(format!(
            "{named} could not work: {}",
            what_it_said(stdout, stderr)
        )));
    }
    Err(ActionError::new(
        "engine_exhausted",
        format!(
            "{named} ran out of its quota, it did not break: {}",
            what_it_said(stdout, stderr)
        ),
    ))
}

/// The line one engine is about to be started with, and what the record of
/// the call needs to know about how it was composed.
struct Prepared {
    invocation: EngineInvocation,
    session: SessionPlan,
    identity: EngineIdentity,
    named: String,
}

/// Composes the line for one engine: which session it continues, where the
/// prompt goes, under which equipment it starts. `set_aside` names the engines
/// already put aside, so the echo can say this one is the fallback.
fn compose(
    candidate: &Candidate,
    spec: &EngineSpec,
    live: Option<&dyn LiveSink>,
    set_aside: &[String],
    record: Option<&Recording<'_>>,
) -> Prepared {
    let bin = &candidate.bin;
    let named = match candidate.id.as_deref() {
        Some(id) => format!("«{id}» (`{bin}`)"),
        None => format!("`{bin}`"),
    };
    // Before composing the line: does this call continue something, or start
    // fresh? It cannot fail — at worst it starts fresh and says so.
    let session = session_plan(candidate, spec.session.as_ref(), spec.blind, record, live, &named);
    let mut args = session
        .args
        .clone()
        .unwrap_or_else(|| candidate.args.clone());
    // The question's text goes where that engine wants it: on stdin for those
    // reading from there, appended to the arguments for those wanting it on
    // the line. It is the one difference between two engines the flow no
    // longer has to know.
    let stdin = match candidate.prompt {
        PromptVia::Stdin => spec.stdin.clone(),
        PromptVia::LastArg => {
            if let Some(text) = &spec.stdin {
                args.push(text.clone());
            }
            None
        }
    };
    if let (Some(live), Some(id)) = (live, candidate.id.as_deref()) {
        if !set_aside.is_empty() {
            live.chunk(
                Pipe::Stderr,
                format!("[sailor] moving on to engine «{id}»\n").as_bytes(),
            );
        }
        if let Some(why) = &candidate.why {
            live.chunk(Pipe::Stderr, format!("[sailor] preferring {why}\n").as_bytes());
        }
    }
    // **SAILOR'S EQUIPMENT, NOT THE TERMINAL'S.** Fault 18: with
    // `env: spec.env.clone()` an engine launched from a flow step inherited
    // the environment of whoever opened the terminal — reading the
    // neighbour's home, while `sailor run` took the same engine into its own.
    // The profile sits **under** `spec.env`: a variable written in the step wins.
    let equipment = current_equipment_for(bin, &spec.env);
    Prepared {
        invocation: EngineInvocation {
            bin: bin.clone(),
            args,
            env: equipment.env,
            workdir: spec.workdir.clone(),
            stdin: stdin.map(String::into_bytes),
            timeout: Duration::from_secs(spec.timeout_secs),
        },
        session,
        identity: equipment.identity,
        named,
    }
}

impl ExternalEngineAction {
    /// Asks one engine: composes its line, says it on the step's echo, starts
    /// it and judges what came back. `set_aside` are the engines already put
    /// aside, and it reaches the error messages: whoever reads a red step must
    /// see the whole chain, not only its last link. The line comes back with
    /// the answer, and on every error raised after the engine was started.
    #[allow(clippy::too_many_arguments)]
    fn ask(
        &self,
        candidate: &Candidate,
        spec: &EngineSpec,
        shape: Option<&ValueSchema>,
        live: Option<&dyn LiveSink>,
        set_aside: &[String],
        solo: bool,
        record: Option<&Recording<'_>>,
        chain: &Chain,
    ) -> Result<(Asked, Ran), ActionError> {
        let prepared = compose(candidate, spec, live, set_aside, record);
        let ran = prepared.invocation.ran();
        if let Some(live) = live {
            live.chunk(Pipe::Stderr, format!("[sailor] {}\n", ran.announce()).as_bytes());
        }
        let started = self.start(
            candidate,
            spec,
            shape,
            live,
            set_aside,
            solo,
            record,
            chain,
            &prepared,
        );
        match started {
            Ok(asked) => Ok((asked, ran)),
            Err(error) => Err(error.having_run(ran)),
        }
    }

    /// Starts the composed line and judges what came back.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        candidate: &Candidate,
        spec: &EngineSpec,
        shape: Option<&ValueSchema>,
        live: Option<&dyn LiveSink>,
        set_aside: &[String],
        solo: bool,
        record: Option<&Recording<'_>>,
        chain: &Chain,
        prepared: &Prepared,
    ) -> Result<Asked, ActionError> {
        let Prepared {
            invocation,
            session,
            identity,
            named,
        } = prepared;
        let seconds = spec.timeout_secs;
        // The instants are taken tight around the call: it is the duration of
        // *this* invocation, not of the step containing it.
        let started_at = now_secs();
        let result = invoke_external_engine_watched_until(
            invocation,
            live,
            &candidate.waits_for_a_person_when,
        );
        let ended_at = now_secs();
        // Usage is read off what the engine said, as its descriptor declares.
        // One declaring nothing leaves everything unknown — not an `if` branch
        // per vendor: the absence of a field in the descriptor.
        let read = |stdout: &str, stderr: &str| match &candidate.declared_usage {
            Some(declared) => models::usage::read_declared(&declared.from.text(stdout, stderr), declared),
            None => Reading::default(),
        };
        // Every branch comes through here: failure and silence too, which is
        // the point — an interrupted call burnt the quota all the same. `said`
        // is the **raw** output, before the wrapper is stripped: that is where
        // an engine names the session it spoke on, in the same place it writes
        // its own tokens.
        let note = |reading: Reading, error_type: Option<&'static str>, said: &str| {
            if let Some(record) = record {
                let session_id = session.session_id(said);
                record_the_call(
                    record,
                    candidate,
                    chain,
                    Spent {
                        reading: this_step_share(record, candidate, session_id.as_deref(), reading),
                        error_type,
                        started_at,
                        ended_at,
                        session_id,
                        identity: identity.clone(),
                        work_kind: spec.kind.clone(),
                        session_mode: session.mode,
                    },
                );
            }
        };
        let outcome = match result {
            EngineResult::Ok { stdout, stderr } => {
                let reading = read(&stdout, &stderr);
                // **SAYING IT CANNOT WORK AND EXITING ZERO ARE COMPATIBLE.**
                // The question «did this engine say it cannot work?» lived
                // only in the `ExitError` branch: here the answer was taken
                // as good, no fallback fired, and the store's row was born
                // with `error_type: None` — the step closing **green** over
                // what was never an answer, which is worse than a lost
                // fallback. Not a textbook case: `CODEX_HOME=<empty> codex
                // exec < /dev/null` answers «No prompt provided via stdin» and
                // exits **zero** (fault 39, measured on this machine); with
                // an `answer_shape` declared the step then died on a shape
                // error, the wrong symptom three rungs further on.
                //
                // **THE DRY PROBE ALREADY HAD THE DISTINCTION**:
                // `judge_dry_run` asks `unusable_when` on `Ok` *and* on
                // `ExitError`. Static check and real run diverged on the same
                // engine — fault 39's shape on another field. Here they
                // converge: **one question, the same two branches**.
                //
                // **THE ROW IS WRITTEN BEFORE TOLERANCE, AS IN THE OTHER
                // BRANCH.** Two different questions, kept apart: the species
                // says **what happened**, `accept` says **what the run makes
                // of it**. With `note(...)` inside the intolerant branch, a
                // step with `accept: ["exit_error"]` writes a `NULL` row over
                // an engine that has just said it cannot work.
                // **AN ANSWER IN THE DECLARED SHAPE IS WORK DONE**, and the
                // words that mean a refusal are not looked for inside it: an
                // engine reading a tree whose own documents discuss quotas
                // prints those words while working, and would refuse itself.
                let answered = reading.answer.clone().unwrap_or_else(|| stdout.clone());
                let in_shape = shape.is_some_and(|shape| shaped_answer(shape, &answered).is_ok());
                let class = if in_shape {
                    None
                } else {
                    candidate.declared_class(&stdout, &stderr)
                };
                let cannot_work = class.is_some();
                // **A SILENCE IS ITS OWN CLASS, AND NEVER A SUCCESS.** With no
                // shape it went out as `status: "ok"` and nothing in it; with
                // one it died on the shape, three steps past the cause. A
                // class the descriptor declares comes first: this is what is
                // left when nobody declared anything. Fault 96.
                let said_nothing = !cannot_work && answered.trim().is_empty();
                // The class is the same as the other branch's: a spent quota
                // or a door shut for another reason, never the exit code.
                note(
                    reading.clone(),
                    class.or(said_nothing.then_some("empty_answer")),
                    &stdout,
                );
                self.set_aside_if_spent(candidate, class, ended_at, &stdout, &stderr);
                // Tolerance comes after, for the same reason as the other
                // branch: a step declaring with `accept` that it keeps this
                // engine's failure wants it as data, and does not want anyone
                // else retrying in its place.
                if cannot_work && !tolerates(&spec.accept, "exit_error") {
                    return engine_cannot_work(named, solo, &stdout, &stderr);
                }
                if said_nothing {
                    return Err(ActionError::new(
                        "empty_answer",
                        format!(
                            "{named} exited with success and said nothing: an empty answer \
                             is not an answer, and nothing downstream can be built on it"
                        ),
                    ));
                }
                // **THE STEP'S OUTPUT DOES NOT CHANGE BECAUSE IT WAS
                // MEASURED.** If the descriptor asked for a wrapper to be told
                // the tokens, the answer is pulled back out of it here and
                // `stdout` is what it was. Measuring must not change what it
                // measures: a downstream flow declaring the shape of its own
                // answer would go red over a measurement it never asked for.
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "ok", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "ok",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::ExitError {
                code,
                stdout,
                stderr,
            } => {
                // Usage is read off the RAW output, before anything else: an
                // engine that exited in error may already have spent, and its
                // tokens must be read where it wrote them.
                let reading = read(&stdout, &stderr);
                // **EXHAUSTED IS NOT BROKEN, AND IT IS LOOKED AT BEFORE THE
                // ROW IS WRITTEN.** This distinction sat ten lines lower and
                // held for a chain and nowhere else: a step with a single
                // engine out of quota was recorded as `exit_error`,
                // indistinguishable from one that breaks. Fault 14 — Claude at
                // its weekly limit stopped a run as if broken, `agy` alive.
                //
                // The species of the row in the store changes accordingly:
                // `exhausted` clears by itself at seven in the morning,
                // `exit_error` does not, and a total that mixes them tells
                // nobody anything.
                //
                // **AND THE SHAPE IS READ BEFORE THE WORDS HERE TOO.** An exit
                // code is a veto over an answer in shape, never a reason to
                // read a refusal inside it: a chain that did would hand the
                // work on and throw an answer away. Fault 95, in the branch
                // its remedy never reached.
                let answered = reading.answer.clone().unwrap_or_else(|| stdout.clone());
                let in_shape = shape.is_some_and(|shape| shaped_answer(shape, &answered).is_ok());
                let class = if in_shape {
                    None
                } else {
                    candidate.declared_class(&stdout, &stderr)
                };
                let exhausted = class.is_some();
                note(reading.clone(), Some(class.unwrap_or("exit_error")), &stdout);
                self.set_aside_if_spent(candidate, class, ended_at, &stdout, &stderr);
                if !tolerates(&spec.accept, "exit_error") {
                    // Tolerance comes first: a step expecting a failure wants
                    // it as data, not someone else retrying in its place.
                    if exhausted {
                        // The consequence is **the same** as for a zero exit,
                        // and lives in one place: two copies of this block
                        // diverged from birth.
                        return engine_cannot_work(named, solo, &stdout, &stderr);
                    }
                    let before = if set_aside.is_empty() {
                        String::new()
                    } else {
                        format!(" (before: {})", each_one_why(set_aside))
                    };
                    return Err(ActionError::new(
                        "engine_exit_error",
                        format!(
                            "{named} {}; {}{before}",
                            how_it_exited(code),
                            what_it_said(&stdout, &stderr)
                        ),
                    ));
                }
                // As in the successful branch: the wrapper comes off, the
                // step's output stays what it was.
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    // An engine that spoke must respect the shape even when
                    // the step forgives its error exit: that tolerance covers
                    // the exit code, not the answer.
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "exit_error", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "exit_error",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::WaitingForAPerson { stdout, stderr } => {
                // Stopped on the words its descriptor declares mean it only
                // waits for a person: a refusal by declaration, so the class is
                // the declared one and never a plain exit error, and the wait
                // it would have cost is the whole reason it was stopped.
                let reading = read(&stdout, &stderr);
                let class = candidate.declared_class(&stdout, &stderr).unwrap_or("exhausted");
                note(reading.clone(), Some(class), &stdout);
                self.set_aside_if_spent(candidate, Some(class), ended_at, &stdout, &stderr);
                if !tolerates(&spec.accept, "exit_error") {
                    return engine_cannot_work(named, solo, &stdout, &stderr);
                }
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "exit_error", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "exit_error",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::TimedOut => {
                // Killed halfway: it said nothing, so there is nothing to
                // read. The row is written all the same, with unknown tokens —
                // the time it ran it truly spent. No session id either, so the
                // session stays unknown even if it had opened one: resuming an
                // interrupted step's session would restart from a context cut
                // at an arbitrary point.
                note(Reading::default(), Some("timed_out"), "");
                // No fallback on a time limit: an engine killed halfway may
                // already have done something, and redoing that work elsewhere
                // would be doing it twice without knowing.
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "engine_timed_out",
                        format!("{named} did not answer within {seconds} seconds and was killed"),
                    ));
                }
                EngineOutcomeJson {
                    status: "timed_out",
                    stdout: String::new(),
                    stderr: String::new(),
                }
            }
            EngineResult::SpawnFailed { reason } => {
                // It never even started: it consumed nothing, but the row says
                // it was tried — without it, a chain falling back to a second
                // engine would look like it had picked that one first.
                note(Reading::default(), Some("spawn_failed"), "");
                if !tolerates(&spec.accept, "spawn_failed") {
                    // Failing to start is the clearest case of «could not
                    // work»: it did nothing, and its descriptor need not
                    // declare it.
                    if !solo {
                        return Ok(Asked::CannotWork(format!(
                            "{named} could not be started: {reason}"
                        )));
                    }
                    return Err(ActionError::new(
                        "engine_spawn_failed",
                        format!("{named} could not be started: {reason}"),
                    ));
                }
                EngineOutcomeJson {
                    status: "spawn_failed",
                    stdout: String::new(),
                    stderr: reason,
                }
            }
        };
        Ok(Asked::Answered(ActionOutcome::Went(json!(outcome))))
    }

    /// An engine that said its quota is spent is set aside for the time its
    /// descriptor declares; without a declared time, or without a home for
    /// the list, it is tried again next time, as before.
    fn set_aside_if_spent(
        &self,
        candidate: &Candidate,
        class: Option<&'static str>,
        now: i64,
        stdout: &str,
        stderr: &str,
    ) {
        let (Some("quota_exhausted"), Some(secs), Some(id), Some(path)) =
            (class, candidate.cooldown_secs, candidate.id.as_deref(), self.cooldowns.as_deref())
        else {
            return;
        };
        // A list that cannot be written costs the next chain one knock: not
        // worth breaking this step over.
        let _ = cooldown::set_aside(path, id, now, secs, &what_it_said(stdout, stderr));
    }

    /// Whether this call may be authorised under the cap the run declares.
    ///
    /// **THE THREE SILENCES ARE DELIBERATE.** No cap, no run, no ledger: each
    /// means the condition cannot be evaluated here, and refusing on any of
    /// them would stop runs nobody capped. The front-level brake in the
    /// executor is unchanged and still there; this is the term it cannot know.
    fn authorise(
        &self,
        candidate: &Candidate,
        spec: &EngineSpec,
        shared: &SharedState,
    ) -> Result<Option<reserve::Held>, ActionError> {
        let (Some(cap), Some(run_id), Some(ledger)) = (
            shared.get(flow::CURRENT_CAP).and_then(|value| value.as_i64()),
            shared
                .get(flow::CURRENT_RUN)
                .and_then(|value| value.as_str().map(str::to_owned)),
            self.ledger.as_ref(),
        ) else {
            return Ok(None);
        };
        let spent = ledger
            .spent_in_run(&run_id)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let next = self.reserve_for(candidate, spec);
        match reserve::admits(cap, &spent, reserve::in_flight(&run_id), &next) {
            Ok(()) => Ok(next.micros().map(|micros| reserve::hold(&run_id, micros))),
            Err(stopped) => Err(ActionError::new(
                "spend_cap_admission",
                reserve::why_it_is_suspended(&stopped),
            )),
        }
    }

    /// The most this call can cost, as a reserve or as the reason there is none.
    ///
    /// The tariffs are those of the model this step asks of this engine. A step
    /// that names none leaves them empty, which prices no token ceiling: the
    /// reserve is then unknown, never zero.
    fn reserve_for(&self, candidate: &Candidate, spec: &EngineSpec) -> reserve::Reserve {
        let Some(ceiling) = &candidate.ceiling else {
            return reserve::Reserve::Unknown(candidate.no_ceiling_because.clone());
        };
        let prices = candidate
            .id
            .as_deref()
            .and_then(|id| spec.model.get(id))
            .and_then(|name| current_price_list().find(name).map(|price| price.micros()))
            .unwrap_or_default();
        reserve::reserve_of(ceiling, &prices)
    }
}

impl Action for ExternalEngineAction {
    /// The fields this action does not know, read off the **real structure**.
    ///
    /// Not a list written by hand beside `EngineSpec`: that would be a second
    /// copy of one truth, and second copies diverge. Here deserialisation is
    /// attempted and whatever landed in `extra` is looked at — so adding a
    /// field to the spec is enough, there is nothing else to update.
    ///
    /// An input that does not deserialise at all yields nothing: at check time
    /// `with` is **partial** by construction — the rest comes from the
    /// dependencies — and complaining about a missing `timeout_secs` would say
    /// something false.
    /// **A STEP THAT NAMES ONLY TOOLS NOBODY CAN ASK A QUESTION OF SPENDS
    /// NOTHING**, and that is read off the descriptors, not off a list here:
    /// an engine declares how it is asked, `cargo` and `npm` declare no such
    /// thing. Unknown either way — no `with`, no chain, no resolver — counts
    /// as paying, which narrows the front and never widens it.
    fn may_spend(&self, declared: Option<&Value>) -> bool {
        let Some(named) = declared.map(crate::spec::engines_named_in) else {
            return true;
        };
        let (Some(tools), false) = (self.tools.as_ref(), named.is_empty()) else {
            return true;
        };
        named.iter().any(|id| tools.ask_recipe(id).is_some())
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<EngineSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.execute_and_report(input, shared)
            .map(|(outcome, _)| outcome)
    }

    /// The line of the engine that answered travels with the answer; with a
    /// chain, the line of the last engine tried travels with the error.
    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        // Where the spend is noted. Built here because `shared` is gone further
        // down, and `None` — nothing noted — when the store or either of the
        // two ids is missing.
        let record = recording_for(&self.ledger, shared);
        // The shape is kept as it was written too: that text, not a rewriting
        // of it, is what must appear in the prompt.
        let written_shape = input.get("answer_shape").map(|shape| {
            serde_json::to_string(shape).expect("a value already in memory always reserialises")
        });
        let mut spec: EngineSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &ENGINE_FAILURES)?;
        // Held until this step returns, and taken down then: the binding is
        // what closes the tree, so it outlives every path out of here.
        let own_tree = tree_of_its_own(&spec, shared, live.clone(), self.ledger.clone())?;
        if let Some(cut) = &own_tree {
            if let Some(live) = live.as_deref() {
                live.chunk(
                    Pipe::Stderr,
                    format!("[sailor] this step works in {}\n", cut.at.display()).as_bytes(),
                );
            }
            spec.workdir = Some(cut.at.to_string_lossy().into_owned());
        }
        // The reason goes where the question is: a step that writes its
        // question in the arguments instead goes without.
        if let Some(told) = spec.after_refusal.take() {
            spec.stdin = spec.stdin.map(|asked| asked_again(&asked, &told));
        }
        if let Some(written) = &written_shape {
            shape_was_asked_for(written, &spec)?;
        }
        // Before spending anything: if none of the engines asked for is usable
        // here, the step stops and says why for each of them.
        let (candidates, refused) = self.candidates(&spec)?;
        if candidates.is_empty() {
            // A single engine that cannot be found stays `tool_unavailable`
            // with the resolver's reason: the commonest case, and that message
            // is already the best there is to give.
            if let [only] = refused.as_slice() {
                if only.unresolved {
                    return Err(ActionError::new("tool_unavailable", only.reason.clone()));
                }
            }
            return Err(ActionError::new(
                "no_usable_engine",
                format!(
                    "none of the engines the step asks for can be used here. {}",
                    each_one_why(&refused.iter().map(Refused::line).collect::<Vec<_>>())
                ),
            ));
        }
        let mut set_aside: Vec<String> = refused.iter().map(Refused::line).collect();
        let shape = spec.answer_shape.as_ref();
        // A step asking for **one** engine has no fallback to make, and must
        // stay identical: the same outcomes, the same messages. The chain
        // changes behaviour where there is a chain, and nowhere else.
        let solo = candidates.len() == 1 && refused.is_empty();
        // The ids of the engines already tried, for the fallback chain written
        // in the row: `set_aside` carries sentences for a person, this carries
        // names a total can group by.
        let mut chain = Chain {
            tried_before: Vec::new(),
            fell_back_from: self.fell_back_from(&spec, &candidates),
        };
        let mut last_ran = None;
        for candidate in &candidates {
            // Astra's condition, before the call and not after it: the only
            // moment at which stopping costs nothing. The reserve is held for
            // as long as the call runs, so a front of steps sharing one cap
            // counts what its siblings have already committed.
            let _held = self.authorise(candidate, &spec, shared)?;
            let (asked, ran) = self.ask(
                candidate,
                &spec,
                shape,
                live.as_deref(),
                &set_aside,
                solo,
                record.as_ref(),
                &chain,
            )?;
            match asked {
                Asked::Answered(outcome) => return Ok((outcome, Some(ran))),
                Asked::CannotWork(why) => {
                    last_ran = Some(ran);
                    set_aside.push(why);
                    if let Some(id) = &candidate.id {
                        chain.tried_before.push(id.clone());
                    }
                }
            }
        }
        let none_could = ActionError::new(
            "no_usable_engine",
            format!(
                "none of the engines the step asks for could work. {}",
                each_one_why(&set_aside)
            ),
        );
        Err(match last_ran {
            Some(ran) => none_could.having_run(ran),
            None => none_could,
        })
    }

    /// It does not declare itself redoable, so it ends up with a person.
    ///
    /// True with a chain too: an engine that declared it cannot work did
    /// nothing, but the one that answered did. The right call for the general
    /// case: behind `bin` and `args` there can be anything — an engine that
    /// already rewrote half a tree, a network request already out — and from
    /// outside it looks like a command that did nothing. Whoever knows their
    /// own engine is idempotent declares it in their own action, as the night
    /// service does: the species belongs to whoever knows the work, not to the
    /// primitive that launches it.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::with_references_resolved;

    /// WHOSE STEP THE TEXT IS: the action asks the factory for the recipient,
    /// naming the step `SharedState` hands it, and what it delivers is what
    /// the engine said.
    #[test]
    fn the_action_asks_the_factory_for_the_step_it_is_running() {
        #[derive(Default)]
        struct Factory {
            asked: std::sync::Mutex<Vec<String>>,
            said: std::sync::Mutex<Vec<u8>>,
        }

        struct Branch(Arc<Factory>);

        impl LiveSink for Branch {
            fn chunk(&self, _pipe: Pipe, bytes: &[u8]) {
                self.0
                    .said
                    .lock()
                    .expect("nobody panics in here")
                    .extend_from_slice(bytes);
            }
        }

        /// The factory sits behind an `Arc` so the test can read what it
        /// recorded once the action is done with it.
        struct FactoryArc(Arc<Factory>);

        impl StepSinks for FactoryArc {
            fn sink_for(&self, step: &str) -> Arc<dyn LiveSink> {
                self.0
                    .asked
                    .lock()
                    .expect("nobody panics in here")
                    .push(step.to_owned());
                Arc::new(Branch(self.0.clone()))
            }
        }

        let factory = Arc::new(Factory::default());
        let action = ExternalEngineAction::new().watched_by(Some(Arc::new(FactoryArc(
            factory.clone(),
        )) as Arc<dyn StepSinks>));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("the-step-that-speaks"));
        let outcome = action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo said-by-the-engine"], "timeout_secs": 10}),
                &shared,
            )
            .expect("the step had to go through");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(
            *factory.asked.lock().expect("nobody panics in here"),
            vec!["the-step-that-speaks".to_owned()]
        );
        let said =
            String::from_utf8_lossy(&factory.said.lock().expect("nobody panics in here")).into_owned();
        assert!(said.contains("said-by-the-engine"), "delivered: {said:?}");
    }

    /// With no step key in the shared state nothing is delivered: a text nobody
    /// can attribute is worse than silence.
    #[test]
    fn no_step_id_means_nobody_is_asked() {
        struct Never;

        impl StepSinks for Never {
            fn sink_for(&self, _step: &str) -> Arc<dyn LiveSink> {
                panic!("no recipient was to be asked for");
            }
        }

        let action =
            ExternalEngineAction::new().watched_by(Some(Arc::new(Never) as Arc<dyn StepSinks>));
        let shared = SharedState::new();
        action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo mute"], "timeout_secs": 10}),
                &shared,
            )
            .expect("the step had to go through all the same");
    }

    /// **A STEP WITH A DEPENDENCY KEEPS RUNNING, AND THERE LIES THE RISK OF
    /// THE NEW CHECK.**
    ///
    /// A step's real input is the previous step's output with `with` laid over
    /// it: `status`, `stdout`, `stderr` and whatever else that step produced
    /// arrive in here. Were `EngineSpec` to refuse them — the obvious road,
    /// `deny_unknown_fields` — no step with a dependency would start again, and
    /// the remedy for fault 20 would be far worse than the fault. This test
    /// holds that door shut.
    #[test]
    fn an_input_carrying_the_previous_step_output_still_runs() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "echo",
            "args": ["done"],
            "timeout_secs": 10,
            // What arrives from the previous step, which this action neither
            // knows nor has to know.
            "status": "ok",
            "stdout": "the output of the step before me\n",
            "stderr": "",
        });

        let outcome = action
            .execute(&input, &SharedState::new())
            .expect("an input carrying the dependency's output has to run");

        let ActionOutcome::Went(output) = outcome else {
            panic!("it had to go through")
        };
        assert_eq!(output["stdout"], "done\n", "and it does its own work");
    }

    /// The twin of the static check, tested **here** where the truth lives: the
    /// same fields ignored at run time are named at check time.
    #[test]
    fn the_action_can_name_the_fields_it_does_not_know() {
        let action = ExternalEngineAction::new();

        let stray = action.unknown_fields(&json!({
            "tool": "claude-code",
            "prompt": "ciao",
            "timeout_secs": 10,
        }));

        assert_eq!(stray, vec!["prompt".to_owned()]);
        assert!(
            action
                .unknown_fields(
                    &json!({"tool": "claude-code", "stdin": "ciao", "timeout_secs": 10})
                )
                .is_empty(),
            "and on a well written input it names nothing"
        );
    }

    // ── the registrable actions ────────────────────────────────────────

    #[test]
    fn the_external_engine_action_reads_its_json_input() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "echo 'answer: 42'"],
            "timeout_secs": 5
        });
        let shared = SharedState::new();
        let outcome = action
            .execute(&input, &shared)
            .expect("the action does not fail");
        let ActionOutcome::Went(output) = outcome else {
            panic!("an engine action that went is always Went")
        };
        assert_eq!(output["status"], "ok");
        assert!(output["stdout"].as_str().unwrap().contains("answer: 42"));
    }

    /// **THE MEASUREMENT THAT COULD HAVE COME OUT DIFFERENT, AND ONCE DID.**
    /// This same input used to close the step `Went` carrying
    /// `status: exit_error`. The mutant that fells the test again is removing
    /// the `return Err` from the action's `ExitError` branch.
    ///
    /// The message is no detail: a broken step writes no typed output, so the
    /// code and what the engine said are either here or lost.
    #[test]
    fn an_engine_that_exits_nonzero_breaks_its_own_step() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "echo a-detail-that-matters 1>&2; exit 3"],
            "timeout_secs": 5
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("an engine that exited in error breaks the step");

        assert_eq!(error.class, "engine_exit_error");
        assert!(error.said.contains("code 3"), "{}", error.said);
        assert!(error.said.contains("a-detail-that-matters"), "{}", error.said);
    }

    /// The other half, without which the first would prove not a choice but a
    /// gap: whoever runs a command **on purpose** to see it fail declares so,
    /// and gets the old behaviour back for that outcome alone.
    #[test]
    fn a_step_can_declare_that_a_nonzero_exit_is_an_acceptable_outcome() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "exit 3"],
            "accept": ["exit_error"],
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("the outcome is declared acceptable")
        else {
            panic!("a tolerated outcome stays a datum")
        };

        assert_eq!(output["status"], "exit_error");
    }

    /// A tolerance declared for an outcome that does not exist would be a
    /// strictness nobody chose: found at once, not on the day it was needed.
    #[test]
    fn a_tolerance_for_an_impossible_outcome_is_refused() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "exit 3"],
            "accept": ["failed"],
            "timeout_secs": 5
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("«failed» is no outcome of an engine");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("exit_error"), "{}", error.said);
    }

    /// A missing binary is the commonest case of a flow written elsewhere, and
    /// it must say **which** binary: the message is the whole repair.
    #[test]
    fn a_binary_that_will_not_start_breaks_the_step_and_names_itself() {
        let action = ExternalEngineAction::new();
        let input = json!({"bin": "/no/binary/here-for-sure", "timeout_secs": 5});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("an engine that will not start breaks the step");

        assert_eq!(error.class, "engine_spawn_failed");
        assert!(
            error.said.contains("/no/binary/here-for-sure"),
            "{}",
            error.said
        );
    }

    #[test]
    fn an_engine_that_never_returns_breaks_the_step_with_its_limit() {
        let action = ExternalEngineAction::new();
        let input = json!({"bin": "sh", "args": ["-c", "exec sleep 60"], "timeout_secs": 1});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("a time limit run out breaks the step");

        assert_eq!(error.class, "engine_timed_out");
        assert!(error.said.contains("within 1 seconds"), "{}", error.said);
    }

    // ── the declared shape of the answer ──────────────────────────────

    /// The step declares the shape **once**, and both things come out of that
    /// field: the text that goes into the prompt and the yardstick the answer
    /// is measured by. Here the fake engine answers well but at length.
    ///
    /// Two things at once, and the second is paid for on every downstream call:
    /// the answer is accepted, and **only** what the shape declares reaches the
    /// next step — no preamble, no extra fields, no raw text.
    #[test]
    fn a_declared_shape_is_enforced_and_only_what_it_declares_is_handed_on() {
        let said = r#"{"paths": ["src/a.rs", "src/b.rs"], "total": 2, "reasoning": "I looked everywhere, and then again"}"#;
        let input = json!({
            "bin": "sh",
            "args": ["-c", format!("printf '%s' '{said}'")],
            "answer_shape": {
                "type": "object",
                "properties": {
                    "paths": {"type": "array", "items": {"type": "string"}},
                    "total": {"type": "number"}
                },
                "required": ["paths", "total"],
                "allow_extra": true
            },
            "stdin": {"$join": ["Answer in this shape: ", {"$json": "/answer_shape"}]},
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect("the answer keeps to the shape")
        else {
            panic!("an engine that answers is always Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(output["answer"]["total"], 2);
        assert_eq!(output["answer"]["paths"][1], "src/b.rs");
        assert!(
            output["answer"].get("reasoning").is_none(),
            "the engine's reasoning must not travel as far as the next step: {output}"
        );
        assert!(
            output.get("stdout").is_none() && output.get("stderr").is_none(),
            "with the raw text beside the answer there would be no saving at all: {output}"
        );
    }

    /// **THE MEASUREMENT THAT COULD HAVE COME OUT DIFFERENT**: the same shape,
    /// an engine answering with a field of the wrong type. The mutant that
    /// fells it is removing the validation after the read — a `total` spelt
    /// out in words would then reach the next step intact.
    #[test]
    fn an_answer_that_does_not_fit_the_shape_breaks_the_step() {
        let said = r#"{"paths": [], "total": "quite a few"}"#;
        let input = json!({
            "bin": "sh",
            "args": ["-c", format!("printf '%s' '{said}'")],
            "answer_shape": {
                "type": "object",
                "properties": {
                    "paths": {"type": "array", "items": {"type": "string"}},
                    "total": {"type": "number"}
                },
                "required": ["paths", "total"],
                "allow_extra": false
            },
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("an engine off the shape has not answered");

        assert_eq!(error.class, "answer_off_shape");
        assert!(error.said.contains("quite a few"), "{}", error.said);
    }

    #[test]
    fn an_answer_that_is_not_json_at_all_breaks_the_step() {
        let input = json!({
            "bin": "sh",
            "args": ["-c", "printf 'sure, leave it with me'"],
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("it is not JSON");

        assert_eq!(error.class, "answer_not_json");
        assert!(error.said.contains("leave it with me"), "{}", error.said);
    }

    /// The engine here is `cat`: it answers exactly what reached its input.
    fn what_the_engine_read(refused: Value, asked: &str) -> String {
        let mut input = json!({
            "bin": "sh",
            "args": ["-c", "cat"],
            "stdin": asked,
            "timeout_secs": 5
        });
        input[flow::AFTER_REFUSAL] = refused;
        let ActionOutcome::Went(output) = ExternalEngineAction::new()
            .execute(&input, &SharedState::new())
            .expect("an engine that repeats what it reads always answers")
        else {
            panic!("an engine that answers is always Went")
        };
        output["stdout"]
            .as_str()
            .expect("the output carries what it said")
            .to_owned()
    }

    fn a_broken_answer(seen: &str) -> Value {
        json!({"check": "answer_shape", "path": "", "rule": "not_json", "seen": seen})
    }

    const THE_QUESTION: &str = "answer in this shape";

    /// **THE SAME QUESTION PLUS THE REASON**, which is the whole difference
    /// between a retry and a lottery. Fault 103.
    #[test]
    fn a_retried_step_asks_the_same_question_with_the_reason_under_it() {
        let read = what_the_engine_read(a_broken_answer("{\"a\": \"b\""), THE_QUESTION);

        assert!(read.starts_with(THE_QUESTION), "{read}");
        assert!(read.contains("answer_shape"), "the check that refused: {read}");
        assert!(read.contains("{\"a\": \"b\""), "where it broke: {read}");
    }

    /// **AND THE REASON IS NOT THE ANSWER.** Fault 103's was 21,193 bytes:
    /// sending it back doubles the price of the attempt meant to save the run.
    #[test]
    fn what_reaches_the_engine_is_the_reason_and_not_the_refused_answer() {
        let refused = "x".repeat(21_193);

        let read = what_the_engine_read(a_broken_answer(&refused), THE_QUESTION);

        assert!(
            read.len() < THE_QUESTION.len() + 400,
            "{} bytes reached the engine, against the {} of the refused answer",
            read.len(),
            refused.len()
        );
    }

    /// **AN EMPTY ANSWER IS NEVER A SUCCESS**, however the engine exited. A
    /// shipped one, asked with the line its own descriptor declares, said
    /// nothing and exited zero: with a shape the step fell three steps past
    /// the cause, without one it closed green over nothing. Fault 96.
    #[test]
    fn an_empty_answer_is_never_a_success() {
        for line in ["exit 0", "printf '  \\n\\t'"] {
            let error = ExternalEngineAction::new()
                .execute(
                    &json!({"bin": "sh", "args": ["-c", line], "timeout_secs": 5}),
                    &SharedState::new(),
                )
                .expect_err("an empty output is not an answer");

            assert_eq!(
                error.class, "empty_answer",
                "on the line «{line}», which exits zero saying nothing: {}",
                error.said
            );
        }
    }

    /// With no schema, «not empty» is the least that makes an answer a
    /// candidate: it does not prove the answer right, and it is all that can
    /// be demanded where nobody declared a shape.
    #[test]
    fn with_no_shape_declared_only_an_empty_answer_is_refused() {
        let action = ExternalEngineAction::new();

        let ActionOutcome::Went(output) = action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo detto"], "timeout_secs": 5}),
                &SharedState::new(),
            )
            .expect("a text that is not empty is handed on as it is")
        else {
            panic!("an engine that answers is always Went")
        };
        assert_eq!(output["stdout"], "detto\n");

        let error = action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "printf ' '"], "timeout_secs": 5}),
                &SharedState::new(),
            )
            .expect_err("one space is not an answer");
        assert_eq!(error.class, "empty_answer", "{}", error.said);
    }

    /// A silence is called a silence: with a shape declared the step said «the
    /// answer is not JSON; it saw «»», which sends the reader looking for a
    /// defect inside an answer instead of at its absence.
    #[test]
    fn a_silence_is_named_a_silence_and_not_a_broken_shape() {
        let input = json!({
            "bin": "sh",
            "args": ["-c", "exit 0"],
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("it said nothing at all");

        assert_eq!(error.class, "empty_answer", "{}", error.said);
    }

    /// Models fence their output: the first fenced block is accepted, even
    /// behind a line of courtesy. Without this rule the shape would be
    /// respected and the step red all the same.
    #[test]
    fn an_answer_inside_a_fence_is_read_anyway() {
        let input = json!({
            "bin": "sh",
            // Raw string: `printf` reads the `\n` sequences, not Rust.
            "args": ["-c", r#"printf 'Here it is:\n```json\n{"total": 7}\n```\n'"#],
            "answer_shape": {
                "type": "object",
                "properties": {"total": {"type": "number"}},
                "required": ["total"],
                "allow_extra": false
            },
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect("the fenced block is read")
        else {
            panic!("an engine that answers is always Went")
        };

        assert_eq!(output["answer"]["total"], 7);
    }

    /// **ASKING AND CHECKING ARE ONE THING.** Here the shape is declared but
    /// never appears in the prompt: the step stops before spending, and it
    /// shows in the missing binary never getting to complain.
    #[test]
    fn a_shape_that_never_reaches_the_prompt_stops_the_step_before_spending() {
        let input = json!({
            "bin": "/no/binary/here-for-sure",
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": "list the paths",
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&input, &SharedState::new())
            .expect_err("the shape was asked of nobody");

        assert_eq!(
            error.class, "shape_not_in_prompt",
            "had it started, the missing binary would have given another error: {}",
            error.said
        );
        assert!(error.said.contains("$json"), "{}", error.said);
    }

    /// The shape holds over what the engine said: a step that tolerates hearing
    /// nothing cannot demand a shape out of that nothing.
    #[test]
    fn a_shape_cannot_live_with_a_tolerance_that_leaves_no_answer() {
        let input = json!({
            "bin": "sh",
            "args": ["-c", "true"],
            "accept": ["timed_out"],
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("the two declarations do not live together");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("timed_out"), "{}", error.said);
    }

    /// **THE HANDOVER, TESTED ON THE ACTION AND NOT ONLY ON THE MODULE.** The
    /// input is what the engine really composes for a step with a dependency:
    /// the previous step's output (`status`, `stdout`, `stderr`) plus the fixed
    /// values of the `with` field, references already resolved as
    /// `flow::step_input` resolves them. Here it is proven the action **uses**
    /// what arrives; that *every* action's input arrives resolved is proven by
    /// the executor, the one place that does it.
    #[test]
    fn a_step_sends_the_previous_engines_answer_into_the_next_one() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "status": "ok",
            "stdout": "=== FOR CODEX ===\ncount the dead hooks",
            "stderr": "",
            "bin": "cat",
            "args": [],
            "stdin": {"$join": ["Run only your own section.\n", {"$from": "/stdout"}]},
            "timeout_secs": 5
        });
        let shared = SharedState::new();

        let ActionOutcome::Went(output) = action
            .execute(&with_references_resolved(input), &shared)
            .unwrap()
        else {
            panic!("an engine that answers is always Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(
            output["stdout"], "Run only your own section.\n=== FOR CODEX ===\ncount the dead hooks",
            "the engine received on its input what the step before it wrote"
        );
    }

    // **A REFERENCE THAT FINDS NOTHING STOPS THE STEP, AND NO LONGER HERE.**
    // Resolution lives in `flow::step_input`, so the step stops **before the
    // action exists**: it is tested where it happens, in
    // `crates/flow/tests/a_reference_reaches_every_action.rs`. Keeping it here
    // too would be two tests of one rule in two places — and this one would go
    // green by calling resolution by hand, measuring the test itself.

    /// The record of an engine step carries the binary and the arguments as
    /// started — with the answer, and with the error when the engine broke.
    #[test]
    fn an_engine_step_reports_the_binary_and_the_arguments_it_started() {
        let answered = json!({"bin": "sh", "args": ["-c", "echo ok"], "timeout_secs": 5});
        let (outcome, ran) = ExternalEngineAction::new()
            .execute_and_report(&with_references_resolved(answered), &SharedState::new())
            .expect("the engine answers");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(ran, Some(Ran::new("sh", ["-c", "echo ok"])));

        let broke = json!({"bin": "sh", "args": ["-c", "exit 3"], "timeout_secs": 5});
        let error = ExternalEngineAction::new()
            .execute_and_report(&with_references_resolved(broke), &SharedState::new())
            .expect_err("an engine that exits red breaks its step");
        assert_eq!(error.class, "engine_exit_error");
        assert_eq!(
            error.ran.as_deref(),
            Some(&Ran::new("sh", ["-c", "exit 3"])),
            "a broken engine step forgot the line it ran"
        );
    }

    /// Whoever watches the step reads the engine's line before the engine has
    /// spoken, on the step's own echo.
    #[test]
    fn the_engine_step_says_what_it_is_about_to_run_before_running_it() {
        struct Recorder(std::sync::Mutex<Vec<(Pipe, Vec<u8>)>>);

        impl LiveSink for Recorder {
            fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
                self.0
                    .lock()
                    .expect("nobody panics here")
                    .push((pipe, bytes.to_vec()));
            }
        }

        struct OneSink(Arc<Recorder>);

        impl StepSinks for OneSink {
            fn sink_for(&self, _step: &str) -> Arc<dyn LiveSink> {
                self.0.clone()
            }
        }

        let recorder = Arc::new(Recorder(std::sync::Mutex::new(Vec::new())));
        let action = ExternalEngineAction::new()
            .watched_by(Some(Arc::new(OneSink(recorder.clone()))));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("ask"));
        action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo ok"], "timeout_secs": 5}),
                &shared,
            )
            .expect("the engine answers");

        let seen = recorder.0.lock().expect("nobody panics here").clone();
        let expected = format!("[sailor] {}\n", Ran::new("sh", ["-c", "echo ok"]).announce());
        let announced = seen
            .iter()
            .position(|(pipe, bytes)| *pipe == Pipe::Stderr && bytes == expected.as_bytes());
        let answered = seen
            .iter()
            .position(|(pipe, bytes)| *pipe == Pipe::Stdout && bytes == b"ok\n");
        assert!(announced.is_some(), "the line was never said: {seen:?}");
        assert!(answered.is_some(), "the engine's own text did not arrive: {seen:?}");
        assert!(
            announced < answered,
            "the line came after the engine had spoken: {seen:?}"
        );
    }
}
