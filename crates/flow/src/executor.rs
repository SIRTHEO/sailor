use crate::record::{truncate_said, Ran, Refusal};
use crate::reference;
use crate::{AttemptRelation, Graph, Outcome, SchemaError, Step, StepRecord, StepSpecies};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// The map every `Action::execute` receives, keyed by a prefix that says who
/// owns the datum. `flow.` is the executor's: it rewrites those keys at every
/// step, so a flow that writes one sees its value overwritten. `workspace.` is
/// written by whoever launches, before the run starts, and holds for the whole
/// run. Two prefixes, so a reader can tell the two owners apart.
pub type SharedState = BTreeMap<String, Value>;

/// The step about to start, written before every `Action::execute`.
///
/// Here and not in the trait signature: an action producing text as it runs
/// must say whose text it is, or in a graph with two live steps nobody
/// attributes it. `execute` never receives the step, and widening the signature
/// would touch every implementor in five crates for a datum few of them need.
pub const CURRENT_STEP: &str = "flow.step";

/// The key under which the executor writes the *run* id, beside the step's.
///
/// It cannot arrive by construction the way the store does: whoever registers
/// the actions holds the store before building the registry, but not yet the
/// run — the `run_id` is born later. Nor may the flow declare it: a data file
/// able to write here could attribute a spend to any run it liked.
pub const CURRENT_RUN: &str = "flow.run";

/// The key under which *the launcher* writes the project root: hence
/// `workspace.` and not `flow.`, per [`SharedState`]. Shared state and not a
/// `{"$root": …}` in the input, because an action that does not read the input —
/// or reads it under a closed schema — would never see that one, while shared
/// state reaches every `Action::execute` by construction. Absent means absent: a
/// flow working wherever it lands does damage, not failure.
pub const WORKSPACE_ROOT: &str = "workspace.root";

/// The key under which the executor writes the run's *spend cap*, when it has
/// one; absent means "no cap", not zero. The executor enforces it and no action
/// needs to know it, except the one that starts another run: an uncapped
/// subflow would annul the cap, so the child must read the remainder where the
/// decision is taken — inside the action. The flow never declares it, or a data
/// file able to write here would raise its own cap.
pub const CURRENT_CAP: &str = "flow.cap_micros";

#[derive(Clone, PartialEq, Eq)]
pub struct ActionError {
    pub class: String,
    pub said: String,
    /// Set when a declared check refused a value: which one, and what it saw.
    /// Boxed: it is the bulk of an error every `Result` in the crate carries.
    pub refusal: Option<Box<Refusal>>,
    /// Set when the action had started a process before failing: the line it
    /// ran, for the record. Boxed for the same reason as the refusal.
    pub ran: Option<Box<Ran>>,
}

impl ActionError {
    pub fn new(class: impl Into<String>, said: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            said: said.into(),
            refusal: None,
            ran: None,
        }
    }

    pub fn refused(mut self, refusal: Refusal) -> Self {
        self.refusal = Some(Box::new(refusal));
        self
    }

    pub fn having_run(mut self, ran: Ran) -> Self {
        self.ran = Some(Box::new(ran));
        self
    }
}

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** With a derived one,
/// every red test showed the fields and hid the sentence. Delegating costs
/// nothing and puts the prose where a failure is read.
impl std::fmt::Debug for ActionError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, out)
    }
}

impl Display for ActionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.class, self.said)
    }
}

impl Error for ActionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStatus {
    Applied(Value),
    NotApplied,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    /// The action knows its own typed result.
    Went(Value),
    /// The action cannot know the result; not a retryable failure.
    Waiting(String),
    /// Not yet: the work cannot be done now, and asking again later may do it.
    ///
    /// It carries the reason and no duration. What counts as long enough is a
    /// decision, and one taken inside a node could not be argued with by the
    /// flow that uses it — so the wait is declared on the step.
    NotYet(String),
}

pub trait Action: Send + Sync {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError>;

    /// `execute`, and beside the outcome the process the action started to
    /// reach it. The executor asks this one: an action whose work is a process
    /// answers it, and every other action answers `execute` through this
    /// default, having started nothing.
    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        self.execute(input, shared).map(|outcome| (outcome, None))
    }

    /// The fields of a hand-written `with` that this action does *not* know.
    ///
    /// Asked only at check time: at run time a step's input is its dependency's
    /// output, where foreign fields are the norm, while a `with` is written by a
    /// person and an unknown field there is a typo that costs a paid call. The
    /// default is silence — an action that cannot list its fields accuses none.
    fn unknown_fields(&self, _declared: &Value) -> Vec<String> {
        Vec::new()
    }

    /// An action with no positive proof is not automatically relaunchable:
    /// `Unknown` keeps the ambiguity instead of duplicating an outside effect.
    fn inspect_effect(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        Ok(EffectStatus::Unknown("effect_not_inspectable".to_owned()))
    }

    /// Whether redoing this action is safe. Not answering hands it to a person:
    /// the defect it guards against is duplicating an effect the world has
    /// already seen, and no default can rule that out on behalf of whoever
    /// wrote the action.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }

    /// Undoes the effect already produced so the step can be redone. Only
    /// meaningful for an action declaring itself `Compensable`: one that
    /// declares it without writing this method fails compensation and lands on
    /// a person, which is the right way for the mistake to show.
    fn compensate(&self, _record: &StepRecord, _shared: &SharedState) -> Result<(), ActionError> {
        Err(ActionError::new(
            "no_compensation",
            "the action declares itself compensable but cannot undo its own effect",
        ))
    }
}

#[derive(Default)]
pub struct ActionRegistry {
    actions: BTreeMap<String, Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn register(
        &mut self,
        name: impl Into<String>,
        action: impl Action + 'static,
    ) -> Option<Box<dyn Action>> {
        self.actions.insert(name.into(), Box::new(action))
    }

    pub fn get(&self, name: &str) -> Option<&dyn Action> {
        self.actions.get(name).map(Box::as_ref)
    }

    /// The registered names, in order.
    ///
    /// Whoever wants to list the actions asks here and keeps no copy: a list
    /// written by hand beside the registry diverges the moment someone adds an
    /// action, and no local check shows it — the report keeps printing a
    /// plausible, stale line.
    pub fn names(&self) -> Vec<&str> {
        self.actions.keys().map(String::as_str).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub outcome: Outcome,
    pub output: Option<Value>,
    pub said: Option<String>,
    pub failure_class: Option<String>,
    pub refusal: Option<Refusal>,
    pub ran: Option<Ran>,
    pub ended_at: i64,
    pub bytes_seen: Option<u64>,
    pub bytes_discarded: Option<u64>,
}

/// What a run has spent, and how much of it stays unknown. It lives in the
/// executor, not the store: it serves to decide, and the executor enforces the
/// cap knowing no stores — it asks its `RecordStore`, of which the real store is
/// one answer. Not an `Option<i64>`, because the cases are three — nothing, this
/// and all of it known, *at least* this — and an `Option` collapses the third
/// into a figure lower than the truth, which is a cap that lets things through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spend {
    /// The sum of the known costs, in currency micro-units.
    pub micros: i64,
    /// The calls recorded for that run, however they went.
    pub calls: i64,
    /// How many of those carry no cost, and `micros` excludes them: this is the
    /// whole of what a cap cannot promise. It is measured only on the costs
    /// engines declare, and codex reports the total of the tokens rather than
    /// the two sides, so its rows carry no cost and never enter the sum. The cap
    /// guarantees over what is known, and the run it stops says how many calls
    /// were outside the count.
    pub calls_without_cost: i64,
    /// The dearest one observed, if at least one is known.
    ///
    /// For deciding *how many steps to open at once*: with N in flight the
    /// worst overshoot is N times this. `None` when no call declared a cost —
    /// and then that arithmetic cannot be done, only invented.
    pub dearest_micros: Option<i64>,
}

impl Spend {
    /// The total is complete: every call said what it cost.
    ///
    /// For whoever must *declare* what they are deciding on. A cap respected
    /// with this `false` is respected only as far as anyone knows.
    pub fn is_complete(&self) -> bool {
        self.calls_without_cost == 0
    }

    /// The three cases, in the shape they are shown to a person. The only way
    /// to ask used to be `is_complete()`, a boolean whoever printed could put
    /// *beside* the number instead of *instead of* it — which is what `sailor
    /// flow cost` did: "1.6674" for a run that had cost 7.2080, with the note
    /// "partial: 3 calls with no known cost" a line below. Returning the case
    /// takes the mistake away from whoever displays it.
    pub fn reading(&self) -> CostReading {
        if !self.is_complete() {
            return CostReading::AtLeast {
                known_micros: self.micros,
                calls: self.calls,
                calls_without_cost: self.calls_without_cost,
            };
        }
        if self.micros == 0 {
            CostReading::Nothing
        } else {
            CostReading::Exact(self.micros)
        }
    }
}

/// How a spend total is to be read.
///
/// The same three cases `Spend` declares. The third is why this type exists:
/// collapsing it onto either of the others — an `Option<i64>`, or a number with
/// a note beside it — hands the reader a figure lower than the truth, and on a
/// run with handed-off steps "lower" was 4.3 times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostReading {
    /// Nothing to show: no call spent, and none is silent.
    Nothing,
    /// The total, and it *is* the total: every call declared its cost.
    Exact(i64),
    /// A floor, not a sum. `known_micros` is what is known; the other two
    /// figures say how much work that number saw nothing of — without them,
    /// "at least 1.67" cannot be told from "1.67 and a cent missing".
    AtLeast {
        known_micros: i64,
        calls: i64,
        calls_without_cost: i64,
    },
}

/// Where it is written that a step started, and how it ended. It takes `&self`,
/// and that is no style detail: with `&mut self` a front of independent steps
/// could not run together, one store being holdable by one thread at a time.
/// Implementors get the mutability they need themselves — `Ledger` has its
/// connection behind a lock already, and its writes are already transactions.
/// `Sync` is asked for the same reason: without it, single file again.
pub trait RecordStore: Sync {
    /// Must make the intent durable before returning to the caller.
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError>;
    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError>;
    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError>;

    /// What that run has spent so far. No default body, deliberately: a default
    /// of `Spend::default()` — zero — would spare existing implementations, but
    /// zero is an assertion, "this run has not spent anything yet", and a cap
    /// fed that by a store which simply does not keep costs never trips and
    /// tells nobody. Whoever implements this declares what they know, even when
    /// the honest answer is "nothing, because I do not record the calls".
    fn spent(&self, run_id: &str) -> Result<Spend, FlowError>;

    /// Whether somebody asked this run to stop. Asked before a front opens,
    /// never in the middle of a step: a step already running finishes, and
    /// the ones that were ready are handed back as not started. The default
    /// is `false` because a store that keeps no such request has none.
    fn halt_requested(&self, _run_id: &str) -> Result<bool, FlowError> {
        Ok(false)
    }
}

/// The store that lives in memory, for tests and for anyone wanting no file.
/// The lock is here because the front runs together: a bare `Vec` sufficed
/// while the trait asked `&mut self` and steps ran in single file, but now that
/// a front starts all at once two threads write in here in the same instant, so
/// the struct gets the mutability itself instead of asking the caller — who
/// could not give it to both.
#[derive(Debug, Default)]
pub struct InMemoryRecordStore {
    records: Mutex<Vec<StepRecord>>,
}

impl InMemoryRecordStore {
    pub fn from_records(records: Vec<StepRecord>) -> Self {
        Self {
            records: Mutex::new(records),
        }
    }

    /// A copy of what is inside right now.
    ///
    /// A copy and not a reference: nothing behind the lock can be lent out
    /// beyond the guard, and lending it anyway would mean reading while another
    /// thread writes.
    pub fn all(&self) -> Vec<StepRecord> {
        self.held().clone()
    }

    /// The guard on the contents. A poisoned lock is a thread that died mid
    /// write: take what is there instead of propagating the panic, because
    /// there are no half-built invariants in here — every write is a `push` or
    /// one assigned field.
    fn held(&self) -> std::sync::MutexGuard<'_, Vec<StepRecord>> {
        self.records.lock().unwrap_or_else(|held| held.into_inner())
    }
}

impl RecordStore for InMemoryRecordStore {
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
        let mut records = self.held();
        if record.outcome.is_some()
            || record.output.is_some()
            || record.said.is_some()
            || record.failure_class.is_some()
            || record.refusal.is_some()
            || record.ran.is_some()
            || record.ended_at.is_some()
            || record.bytes_seen.is_some()
            || record.bytes_discarded.is_some()
        {
            return Err(FlowError::InvalidRecord(
                "a started record already contains closing fields".to_owned(),
            ));
        }
        let duplicate = records.iter().any(|found| {
            found.run_id == record.run_id
                && found.step_id == record.step_id
                && found.attempt == record.attempt
        });
        if duplicate {
            return Err(FlowError::DuplicateAttempt {
                step: record.step_id,
                attempt: record.attempt,
            });
        }
        let greatest_epoch = records
            .iter()
            .filter(|found| found.run_id == record.run_id && found.step_id == record.step_id)
            .map(|found| found.epoch)
            .max();
        if greatest_epoch.is_some_and(|epoch| record.epoch <= epoch) {
            return Err(FlowError::StaleEpoch {
                step: record.step_id,
                epoch: record.epoch,
            });
        }
        records.push(record);
        Ok(())
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        mut completion: Completion,
    ) -> Result<(), FlowError> {
        let mut records = self.held();
        let greatest_epoch = records
            .iter()
            .filter(|found| found.run_id == run_id && found.step_id == step_id)
            .map(|found| found.epoch)
            .max();
        if greatest_epoch != Some(epoch) {
            return Err(FlowError::StaleEpoch {
                step: step_id.to_owned(),
                epoch,
            });
        }
        let record = records
            .iter_mut()
            .find(|found| {
                found.run_id == run_id
                    && found.step_id == step_id
                    && found.attempt == attempt
                    && found.epoch == epoch
            })
            .ok_or_else(|| FlowError::MissingAttempt {
                step: step_id.to_owned(),
                attempt,
            })?;
        if record.outcome.is_some() {
            return Err(FlowError::AlreadyClosed {
                step: step_id.to_owned(),
                attempt,
            });
        }
        if let Some(said) = completion.said.take() {
            completion.said = Some(truncate_said(said));
        }
        record.outcome = Some(completion.outcome);
        record.output = completion.output;
        record.said = completion.said;
        record.failure_class = completion.failure_class;
        record.refusal = completion.refusal;
        record.ran = completion.ran;
        record.ended_at = Some(completion.ended_at);
        record.bytes_seen = completion.bytes_seen;
        record.bytes_discarded = completion.bytes_discarded;
        Ok(())
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        Ok(self
            .held()
            .iter()
            .filter(|record| record.run_id == run_id)
            .cloned()
            .collect())
    }

    /// This store records no calls, so it knows nothing about spending, and
    /// here zero is the true answer rather than a fallback: no call written, no
    /// cost, nothing unknown. Declaring a cap on top of this store gives a cap
    /// that never trips — correct, but worth knowing: the tests that measure
    /// the cap use a store that does keep costs.
    fn spent(&self, _run_id: &str) -> Result<Spend, FlowError> {
        Ok(Spend::default())
    }
}

/// What time it is. `&self` and `Sync` for the same reason as the store: two
/// steps running together ask for the time together.
pub trait Clock: Sync {
    fn now(&self) -> Result<i64, FlowError>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<i64, FlowError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .map_err(|error| FlowError::Clock(error.to_string()))
    }
}

pub trait ProcessProbe {
    fn is_running(&self, record: &StepRecord) -> Result<bool, FlowError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Ready(Vec<String>),
    Running(Vec<String>),
    Waiting(Vec<String>),
    /// Nothing is ready *now*, and something becomes ready later.
    ///
    /// Apart from `Waiting`, which is a run somebody has to come and take, and
    /// apart from `Running`, which is a run in flight. The executor never waits
    /// on this: it ends the run and hands back `due_at`, so whoever beats does
    /// not have to reread the records to work out when to come back.
    NotYet {
        steps: Vec<String>,
        /// The nearest instant at which one of them becomes ready.
        due_at: i64,
    },
    Stopped(Vec<String>),
    Failed(Vec<String>),
    /// The run stopped itself rather than go over the spend cap: its own word,
    /// neither `Stopped` nor `Failed`. `Failed` would say something broke, and a
    /// nightly flow touching its cap every night would look broken every night
    /// until nobody looked. `Stopped` already means a step the store holds
    /// still; here it is the run that stopped, and for a reason that can be
    /// read in money.
    CapReached(SpendStop),
    /// Somebody asked the run to stop, and it did before opening this front.
    /// The steps carried are the ones that were ready and did not start: not
    /// faults, and a resume finds them where they were.
    Halted(Vec<String>),
    Complete,
}

/// Why the run stopped, with the numbers to judge it by.
///
/// It carries the data, not the sentence. The sentence is composed by whoever
/// displays — the terminal in one line, the window in a panel — and in two
/// languages if that is ever needed. A message already formatted in here would
/// force both of them to take it apart to rebuild it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendStop {
    /// The cap declared for this run, in micro-units.
    pub cap_micros: i64,
    /// What is recorded as spent, and how much of that stays unknown.
    pub spent: Spend,
    /// The steps that were ready and did not start. Not faults: they are still
    /// to do, and a resume under a higher cap finds them there.
    pub not_started: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub decisions: Vec<Decision>,
    pub shared: SharedState,
}

/// How a run ended, and whether whoever launched it can call it satisfied:
/// `cap_reached` and `waiting` are not faults, and not "it went" either. A
/// third caller needed this translation — the subflow step, which lives here
/// and cannot see into `registry` — so rather than copy it a third time it
/// moved beside `Decision`, the type it translates. `registry::execution_status`
/// stays the public name its callers already use, and calls this.
pub fn run_status(execution: &Execution) -> (&'static str, bool) {
    match execution.decisions.last() {
        Some(Decision::Complete) => ("complete", true),
        Some(Decision::Waiting(_)) => ("waiting", false),
        Some(Decision::NotYet { .. }) => ("not_yet", false),
        Some(Decision::Stopped(_)) => ("stopped", false),
        Some(Decision::Failed(_)) => ("failed", false),
        Some(Decision::CapReached(_)) => ("cap_reached", false),
        Some(Decision::Halted(_)) => ("stopped", false),
        Some(Decision::Ready(_)) | Some(Decision::Running(_)) | None => ("incomplete", false),
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub run_id: String,
    pub root_inputs: BTreeMap<String, Value>,
    pub gates: Vec<String>,
    pub shared: SharedState,
    /// What this run may spend, in currency micro-units.
    ///
    /// `None` means "no cap declared", not zero — the two are opposites.
    /// `Some(0)` is a flow that must not spend anything and stops before the
    /// first paid call; `None` is a flow nobody set a limit on. The default is
    /// `None`: a cap appearing by itself would stop runs nobody asked to stop.
    pub spend_cap_micros: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reconciliation {
    pub closed_as_went: Vec<String>,
    pub closed_as_broke: Vec<String>,
    pub closed_as_waiting: Vec<String>,
    pub still_running: Vec<String>,
    /// The steps whose effect was undone before reopening them. Not a bucket
    /// apart from the others: they are also in `closed_as_broke`, which is
    /// where "becomes ready again" reads. Here something else reads — something
    /// was undone in the world, and the reader must know.
    pub compensated: Vec<String>,
}

pub struct ReconciliationRequest<'a> {
    pub graph: &'a Graph,
    pub run_id: &'a str,
    pub store: &'a mut dyn RecordStore,
    pub actions: &'a ActionRegistry,
    pub shared: &'a SharedState,
    pub processes: &'a dyn ProcessProbe,
    pub clock: &'a dyn Clock,
}

pub trait Executor {
    fn execute(
        &self,
        graph: &Graph,
        request: ExecutionRequest,
        store: &dyn RecordStore,
        actions: &ActionRegistry,
        clock: &dyn Clock,
    ) -> Result<Execution, FlowError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessExecutor;

impl InProcessExecutor {
    /// What is to be done on this run, as of the clock's `now`.
    ///
    /// The clock is a parameter and not a detail: since a step can be postponed
    /// to an instant, "what is ready" is a question with a time in it, and one
    /// answered without a clock could only be answered wrongly.
    pub fn decision(
        &self,
        graph: &Graph,
        run_id: &str,
        store: &dyn RecordStore,
        clock: &dyn Clock,
    ) -> Result<Decision, FlowError> {
        let records = store.records(run_id)?;
        decision_from(graph, &records, clock.now()?)
    }

    pub fn reconcile(
        &self,
        request: ReconciliationRequest<'_>,
    ) -> Result<Reconciliation, FlowError> {
        let ReconciliationRequest {
            graph,
            run_id,
            store,
            actions,
            shared,
            processes,
            clock,
        } = request;
        let records = store.records(run_id)?;
        let mut report = Reconciliation::default();
        for record in records.iter().filter(|record| record.outcome.is_none()) {
            if processes.is_running(record)? {
                report.still_running.push(record.step_id.clone());
                continue;
            }
            let resolved = match graph.step(&record.step_id) {
                None => Err(ActionError::new("unknown_step", &record.step_id)),
                Some(step) => match actions.get(&step.action) {
                    None => Err(ActionError::new("unknown_action", &step.action)),
                    Some(action) => Ok((step, action)),
                },
            };
            let action = resolved.as_ref().ok().map(|(_, action)| *action);
            let inspected = match resolved {
                Err(error) => Err(error),
                Ok((step, action)) => action.inspect_effect(record, shared).and_then(|status| {
                    if let EffectStatus::Applied(output) = &status {
                        step.output_schema.validate(output).map_err(|error| {
                            ActionError::new("invalid_recovered_output", error.to_string())
                        })?;
                    }
                    Ok(status)
                }),
            };
            let now = clock.now()?;
            let (completion, bucket) = match inspected {
                Ok(EffectStatus::Applied(output)) => (
                    closed(Outcome::Went, Some(output), None, None, now),
                    &mut report.closed_as_went,
                ),
                Ok(EffectStatus::NotApplied) => (
                    closed(Outcome::Broke, None, None, Some("process_disappeared"), now),
                    &mut report.closed_as_broke,
                ),
                // The effect is unknown: here, and only here, the step's species
                // decides. Without it the one safe choice was "waiting" — and a
                // waiting step never becomes ready again (`decision_from`), so
                // a resume saw the interrupted step and never relaunched it.
                Ok(EffectStatus::Unknown(reason)) => {
                    match species_for(record, action) {
                        StepSpecies::Repeatable => (
                            closed(
                                Outcome::Broke,
                                None,
                                Some(reason),
                                Some("repeatable_after_unknown_effect"),
                                now,
                            ),
                            &mut report.closed_as_broke,
                        ),
                        StepSpecies::Compensable => {
                            let compensation = action.map_or_else(
                                || {
                                    Err(ActionError::new(
                                        "unknown_action",
                                        "no action to undo the effect with",
                                    ))
                                },
                                |action| action.compensate(record, shared),
                            );
                            match compensation {
                                Ok(()) => {
                                    report.compensated.push(record.step_id.clone());
                                    (
                                        closed(
                                            Outcome::Broke,
                                            None,
                                            Some(reason),
                                            Some("compensated_then_retry"),
                                            now,
                                        ),
                                        &mut report.closed_as_broke,
                                    )
                                }
                                // Declared compensation that fails leaves the
                                // world half done: this is the case where a
                                // person is genuinely needed.
                                Err(error) => (
                                    closed(
                                        Outcome::Waiting,
                                        None,
                                        Some(error.said),
                                        Some(error.class.as_str()),
                                        now,
                                    ),
                                    &mut report.closed_as_waiting,
                                ),
                            }
                        }
                        StepSpecies::HandToHuman => (
                            closed(
                                Outcome::Waiting,
                                None,
                                Some(reason),
                                Some("effect_unknown"),
                                now,
                            ),
                            &mut report.closed_as_waiting,
                        ),
                    }
                }
                Err(error) => (
                    closed(
                        Outcome::Waiting,
                        None,
                        Some(error.said),
                        Some(error.class.as_str()),
                        now,
                    ),
                    &mut report.closed_as_waiting,
                ),
            };
            store.close(
                run_id,
                &record.step_id,
                record.attempt,
                record.epoch,
                completion,
            )?;
            bucket.push(record.step_id.clone());
        }
        Ok(report)
    }
}

impl Executor for InProcessExecutor {
    fn execute(
        &self,
        graph: &Graph,
        mut request: ExecutionRequest,
        store: &dyn RecordStore,
        actions: &ActionRegistry,
        clock: &dyn Clock,
    ) -> Result<Execution, FlowError> {
        let mut decisions = Vec::new();
        // The root is read once and held by value: it stays the same for the
        // whole run, and a reference into `request.shared` would stop the
        // executor writing the run id there a few lines below.
        let root: Option<PathBuf> = request
            .shared
            .get(WORKSPACE_ROOT)
            .and_then(Value::as_str)
            .map(PathBuf::from);
        loop {
            let records = store.records(&request.run_id)?;
            let decision = decision_from(graph, &records, clock.now()?)?;
            decisions.push(decision.clone());
            let Decision::Ready(front) = decision else {
                return Ok(Execution {
                    decisions,
                    shared: request.shared,
                });
            };

            // A stop asked by hand is read before the front opens, the only
            // moment it costs nothing to honour: a step already at work has
            // already paid, and the engine cannot take it back.
            if store.halt_requested(&request.run_id)? {
                decisions.push(Decision::Halted(front));
                return Ok(Execution {
                    decisions,
                    shared: request.shared,
                });
            }

            // The cap is checked before opening, not after spending: a step
            // that discovers halfway through that it went over has already
            // paid. The only moment where stopping costs nothing is before the
            // front opens. The comparison is `>=`, not `>`, because with `>` a
            // cap of zero would let the first call through — precisely the case
            // where someone is saying "this flow must not spend anything".
            let mut at_once = AT_ONCE;
            if let Some(cap) = request.spend_cap_micros {
                let spent = store.spent(&request.run_id)?;
                if spent.micros >= cap {
                    decisions.push(Decision::CapReached(SpendStop {
                        cap_micros: cap,
                        spent,
                        not_started: front,
                    }));
                    return Ok(Execution {
                        decisions,
                        shared: request.shared,
                    });
                }
                at_once = how_many_fit(cap - spent.micros, spent.dearest_micros);
            }

            // The epoch belongs to the front, not to the step: computed once
            // here, before any step opens, and the whole wave carries it. Each
            // step used to compute it from the same snapshot of the records and
            // it came out equal anyway; the difference is that it is now
            // declared instead of coincidental, and whoever reads a run sees
            // those steps started together because they share one epoch.
            let epoch = records.iter().map(|record| record.epoch).max().unwrap_or(0) + 1;
            // All are opened first and executed after. Opening is short and
            // orderly, execution long and concurrent: keeping them apart makes
            // the order steps appear in the store the graph's, not the order in
            // which threads win the race, and leaves each step's closing in its
            // own thread the moment it finishes, so a watcher sees it arrive
            // when it happens.
            let mut opened: Vec<Opened<'_>> = Vec::with_capacity(front.len());
            for step_id in front {
                let step = graph
                    .step(&step_id)
                    .ok_or_else(|| FlowError::UnknownStep(step_id.clone()))?;
                let previous = latest_for(step, &records);
                let attempt = previous.map_or(1, |record| record.attempt + 1);
                // A reference that finds nothing breaks the step, not the run.
                // Resolved inside the actions, a dead pointer came back as an
                // `ActionError` and was stored like any broken step: the run
                // reached `Failed([it])`, a watcher saw which, a resume knew
                // where to restart. Resolved here, `?` would abort `execute`
                // opening and closing nothing — a step that never existed.
                let input = match step_input(
                    graph,
                    step,
                    &request.root_inputs,
                    &records,
                    root.as_deref(),
                ) {
                    Ok(input) => input,
                    Err(FlowError::Action(error)) => {
                        // The intent is written with the input as the step
                        // received it, references included: that is what a
                        // reader needs to see to know which pointer to fix.
                        let mut started = StepRecord::started(
                            &request.run_id,
                            &step.id,
                            attempt,
                            epoch,
                            step.deps.clone(),
                            composed_input(graph, step, &request.root_inputs, &records)
                                .unwrap_or(Value::Null),
                            request.gates.clone(),
                            clock.now()?,
                        );
                        started.attempt_relation = attempt_relation(&records, &started);
                        started.held_by_pid = Some(std::process::id());
                        started.species = actions.get(&step.action).map(|action| action.species());
                        store.append_started(started)?;
                        store.close(
                            &request.run_id,
                            &step.id,
                            attempt,
                            epoch,
                            broke(error, clock.now()?),
                        )?;
                        continue;
                    }
                    // The other composition defects — an absolute path inside
                    // the flow, a missing root — belong to the *flow*, not to a
                    // step: resuming the run does not repair them, so stopping
                    // is the right answer.
                    Err(other) => return Err(other),
                };
                let StepInput {
                    value: input,
                    runs: condition_met,
                } = input;
                step.input_schema.validate(&input)?;
                // `step_input` already decided the condition, and it is the only
                // place that could: whether references resolve depends on it.
                // Re-evaluating here would mean two answers to one question,
                // on two different values.
                let action = if condition_met {
                    Some(
                        actions
                            .get(&step.action)
                            .ok_or_else(|| FlowError::UnknownAction(step.action.clone()))?,
                    )
                } else {
                    None
                };
                let mut started = StepRecord::started(
                    &request.run_id,
                    &step.id,
                    attempt,
                    epoch,
                    step.deps.clone(),
                    input.clone(),
                    request.gates.clone(),
                    clock.now()?,
                );
                started.attempt_relation = attempt_relation(&records, &started);
                // This process holds the step, by definition of an in-process
                // executor: the pid is written BEFORE the effect, with the
                // intent, or it is of no use to a resume.
                started.held_by_pid = Some(std::process::id());
                started.species = action.map(|action| action.species());
                store.append_started(started)?;
                opened.push(Opened {
                    step,
                    input,
                    attempt,
                    action,
                });
            }

            // The run enters shared state once and for all: it is the same for
            // every step. The step id belongs to each one, and each gets it in
            // its own copy — see `run_one`.
            request.shared.insert(
                CURRENT_RUN.to_owned(),
                Value::String(request.run_id.clone()),
            );
            // The cap goes in beside the run, and only if there is one: the
            // absent key is "no cap declared", which is not `Some(0)`. Without
            // this line a child flow would run uncapped under a capped parent.
            if let Some(cap) = request.spend_cap_micros {
                request
                    .shared
                    .insert(CURRENT_CAP.to_owned(), Value::from(cap));
            }

            // The front runs together — see fault 7. A `for` used to walk the
            // ready steps one after the other: two independent six-second steps
            // took twelve, three took eighteen, linear, with the machine at 0%
            // processor for the whole time. A front is one decision even when
            // an executor walks it in order, and single file left "use the
            // machine" with nothing to stand on.

            // In groups, and the width comes from the money. A wide front is
            // rare in a hand-written graph, but when it happens the steps are
            // agents, not sums: twenty at once would mean twenty processes and
            // twenty paid calls nobody asked for. `AT_ONCE` is the ceiling, not
            // the number — under a cap the width narrows as the remainder falls
            // (see `how_many_fit`); with no cap it stays what it always was.
            let mut failure: Option<FlowError> = None;
            for group in opened.chunks(at_once) {
                let outcomes: Vec<Result<(), FlowError>> = std::thread::scope(|scope| {
                    let handles: Vec<_> = group
                        .iter()
                        .map(|work| {
                            let shared = &request.shared;
                            let run_id = request.run_id.as_str();
                            scope.spawn(move || run_one(work, run_id, epoch, shared, store, clock))
                        })
                        .collect();
                    handles
                        .into_iter()
                        .map(|handle| match handle.join() {
                            Ok(result) => result,
                            // A thread that panics must not take the run away
                            // silently: it becomes a fault of the step, saying
                            // that it happened here.
                            Err(_) => Err(FlowError::Store(
                                "a step of the front died while running".to_owned(),
                            )),
                        })
                        .collect()
                });
                for outcome in outcomes {
                    if let Err(error) = outcome {
                        // Keep the first and carry on to the end of the group:
                        // leaving at once would leave the wave's already-open
                        // steps unclosed, and on resume they would look held by
                        // a living process.
                        failure.get_or_insert(error);
                    }
                }
                if failure.is_some() {
                    break;
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
        }
    }
}

/// The *ceiling* on how many steps run together: under a cap the number itself
/// comes from [`how_many_fit`]. Not a technical limit — the machine would hold
/// more; engine quotas and the watcher's patience will not. Four is enough to
/// erase the wait on a normal front, which in the flows written so far is two
/// or three steps, and few enough not to open a dozen paid conversations for a
/// run nobody is supervising. Public because `for_each` opens its children
/// under the same ceiling: one width for the machine, declared once.
pub const AT_ONCE: usize = 4;

/// How many steps can be opened at once with this remainder.
///
/// A cap cannot be respected with a wide front: four calls start in the same
/// instant, none of them aware of the others, and by the time the first records
/// its cost the other three have already spent. The worst overshoot is however
/// many are in flight, so the width is arithmetic on the remainder.
fn how_many_fit(remaining_micros: i64, dearest_micros: Option<i64>) -> usize {
    // The measure is the dearest call seen *in this run*: the worst case
    // observed, never an average — an average would let it overshoot every time
    // the next call came in above it, which is half of them. Other runs are left
    // out on purpose: a flow calling a small model must not narrow because
    // yesterday a different flow called a big one.
    let Some(dearest) = dearest_micros.filter(|dearest| *dearest > 0) else {
        // Nothing known narrows nothing. Returning 1 "to be careful" is an
        // arbitrary choice in disguise: it would serialise every capped run
        // forever on the strength of a number that does not exist. Stay at the
        // ceiling — the run stops at the next front's check, where the cap works.
        return AT_ONCE;
    };
    // The remainder is positive by construction: the caller has already checked
    // the spend has not reached the cap. Integer division truncates downwards,
    // which is the right way — three and a half calls of margin are three.
    let fit = (remaining_micros / dearest).clamp(1, AT_ONCE as i64);
    fit as usize
}

/// A step already opened in the store, waiting to be executed.
struct Opened<'a> {
    step: &'a Step,
    input: Value,
    attempt: u32,
    action: Option<&'a dyn Action>,
}

/// Runs a step and closes it, in its own thread. The shared state is a copy,
/// and that is the delicate point of the whole thing: an action that produces
/// text, or records a spend, asks it which step is current (`CURRENT_STEP`).
/// One key sufficed while only one step was ever live. With two, that key holds
/// one value and one step's text and *costs* land on the other — silently, with
/// nothing going red. So the key stays one and the map is each thread's own.
fn run_one(
    work: &Opened<'_>,
    run_id: &str,
    epoch: u64,
    shared: &SharedState,
    store: &dyn RecordStore,
    clock: &dyn Clock,
) -> Result<(), FlowError> {
    let step = work.step;
    let mut mine = shared.clone();
    mine.insert(CURRENT_STEP.to_owned(), Value::String(step.id.clone()));

    let completion = match work.action {
        None => closed(Outcome::Skipped, None, None, None, clock.now()?),
        Some(action) => match action.execute_and_report(&work.input, &mine) {
            Ok((outcome, ran)) => {
                let mut completion = match outcome {
                    ActionOutcome::Went(output) => match step.output_schema.validate(&output) {
                        Ok(()) => closed(Outcome::Went, Some(output), None, None, clock.now()?),
                        Err(error) => broke(
                            ActionError::new("invalid_output", error.to_string())
                                .refused(error.refused_by("output_schema")),
                            clock.now()?,
                        ),
                    },
                    ActionOutcome::Waiting(reason) => {
                        closed(Outcome::Waiting, None, Some(reason), None, clock.now()?)
                    }
                    // No `failure_class`: not yet is not a failure, and a class
                    // here would put an ordinary poll into every count of what
                    // went wrong.
                    ActionOutcome::NotYet(reason) => {
                        closed(Outcome::NotYet, None, Some(reason), None, clock.now()?)
                    }
                };
                // Whatever the outcome says of it, that process ran: an output
                // the schema refused was still produced by it.
                completion.ran = ran;
                completion
            }
            Err(error) => broke(error, clock.now()?),
        },
    };
    store.close(run_id, &step.id, work.attempt, epoch, completion)
}

#[derive(Clone, PartialEq, Eq)]
pub enum FlowError {
    Store(String),
    Clock(String),
    InvalidRecord(String),
    DuplicateAttempt {
        step: String,
        attempt: u32,
    },
    MissingAttempt {
        step: String,
        attempt: u32,
    },
    AlreadyClosed {
        step: String,
        attempt: u32,
    },
    StaleEpoch {
        step: String,
        epoch: u64,
    },
    UnknownStep(String),
    UnknownAction(String),
    MissingOutput(String),
    Schema(SchemaError),
    Action(ActionError),
    /// A location field with an absolute path written inside the flow.
    AbsolutePath {
        step: String,
        field: String,
        value: String,
    },
    /// A step needs the project root and nobody carried one in.
    NoWorkspaceRoot {
        step: String,
        field: String,
        value: String,
    },
}

impl From<SchemaError> for FlowError {
    fn from(value: SchemaError) -> Self {
        Self::Schema(value)
    }
}

impl From<ActionError> for FlowError {
    fn from(value: ActionError) -> Self {
        Self::Action(value)
    }
}

impl std::fmt::Debug for FlowError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, out)
    }
}

impl Display for FlowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FlowError::Store(error) => write!(formatter, "record store: {error}"),
            FlowError::Clock(error) => write!(formatter, "clock: {error}"),
            FlowError::InvalidRecord(error) => write!(formatter, "invalid record: {error}"),
            FlowError::DuplicateAttempt { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} already exists")
            }
            FlowError::MissingAttempt { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} does not exist")
            }
            FlowError::AlreadyClosed { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} is already closed")
            }
            FlowError::StaleEpoch { step, epoch } => {
                write!(formatter, "step {step} epoch {epoch} is stale")
            }
            FlowError::UnknownStep(step) => write!(formatter, "unknown step {step}"),
            FlowError::UnknownAction(action) => write!(formatter, "unknown action {action}"),
            FlowError::MissingOutput(step) => write!(formatter, "step {step} has no typed output"),
            FlowError::Schema(error) => Display::fmt(error, formatter),
            FlowError::Action(error) => Display::fmt(error, formatter),
            FlowError::AbsolutePath { step, field, value } => write!(
                formatter,
                "step {step} declares \"{field}\" with an absolute path ({value}): \
                 a flow must not know where the project is, or it runs in one place only. \
                 Remove it with \"sailor flow relocate\""
            ),
            FlowError::NoWorkspaceRoot { step, field, value } => write!(
                formatter,
                "step {step} declares \"{field}\" as relative ({value}) but there is no \
                 project root: no {} walking up from where you launched. \
                 Create one with \"sailor workspace init\"",
                crate::workspace::MARKER
            ),
        }
    }
}

impl Error for FlowError {}

fn closed(
    outcome: Outcome,
    output: Option<Value>,
    said: Option<String>,
    failure_class: Option<&str>,
    ended_at: i64,
) -> Completion {
    Completion {
        outcome,
        output,
        said,
        failure_class: failure_class.map(str::to_owned),
        refusal: None,
        ran: None,
        ended_at,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

/// A step broken by its action. When a declared check refused a value, the
/// sentence a person reads opens with which check and what it saw, and the
/// refusal itself travels beside the class so it can be counted.
fn broke(error: ActionError, ended_at: i64) -> Completion {
    let said = match &error.refusal {
        Some(refusal) => format!("{}\n{}", refusal.explain(), error.said),
        None => error.said,
    };
    Completion {
        outcome: Outcome::Broke,
        output: None,
        said: Some(said),
        failure_class: Some(error.class),
        refusal: error.refusal.map(|refusal| *refusal),
        ran: error.ran.map(|ran| *ran),
        ended_at,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

/// The species of an opened step. The record beats the action: it is what held
/// when the step started, and an action rewritten since cannot change the
/// judgement on an effect produced by the earlier version. The action answers
/// only for records written before species existed; failing that, hand it to a
/// person.
fn species_for(record: &StepRecord, action: Option<&dyn Action>) -> StepSpecies {
    record
        .species
        .or_else(|| action.map(|action| action.species()))
        .unwrap_or(StepSpecies::HandToHuman)
}

/// The smallest gap the engine can tell from "now": its clock counts whole
/// seconds. Not a policy — the policy is `Step::ask_again_after_secs`, and this
/// is what stands in for it when a step declares none, so that "not yet" cannot
/// mean "again immediately" and spin the executor on one step.
const NEXT_INVOCATION_AT_THE_EARLIEST: i64 = 1;

/// When a closed record may be tried again. A record with no closing instant
/// counts as closed now, so the step is postponed rather than tried at once:
/// the way to be wrong here is to spin, not to wait a beat too long.
fn ready_again_at(record: &StepRecord, now: i64, wait_secs: i64) -> i64 {
    record.ended_at.unwrap_or(now).saturating_add(wait_secs)
}

/// How many times this step has broken. **Not the attempt number**: a step that
/// answers `NotYet` opens an attempt without failing, so counting the ordinal
/// would let ordinary polling exhaust `max_attempts`. On a flow with no `NotYet`
/// the two are the same number, which is why nothing existing changes.
fn times_broken(step: &Step, records: &[StepRecord]) -> u32 {
    records
        .iter()
        .filter(|record| record.step_id == step.id && record.outcome == Some(Outcome::Broke))
        .count() as u32
}

fn decision_from(graph: &Graph, records: &[StepRecord], now: i64) -> Result<Decision, FlowError> {
    let mut ready = Vec::new();
    let mut running = Vec::new();
    let mut waiting = Vec::new();
    let mut not_yet: Vec<(String, i64)> = Vec::new();
    let mut stopped = Vec::new();
    let mut failed = Vec::new();
    for step in graph.steps() {
        let latest = latest_for(step, records);
        // Ready or later, for a step that has closed and may be tried again.
        // Not being ready is never a reason to drop it: it goes in `not_yet`
        // with the instant it comes back, which is what tells this apart from
        // `Waiting`, where nothing comes back.
        let mut ready_or_later = |due: Option<i64>| match due {
            Some(due) if due > now => not_yet.push((step.id.clone(), due)),
            _ => {
                if dependencies_satisfied(graph, step, records) {
                    ready.push(step.id.clone());
                }
            }
        };
        match latest.and_then(|record| record.outcome) {
            Some(Outcome::Went) => continue,
            None if latest.is_some() => running.push(step.id.clone()),
            Some(Outcome::Waiting) => waiting.push(step.id.clone()),
            Some(Outcome::Stopped) => stopped.push(step.id.clone()),
            Some(Outcome::Skipped) => continue,
            Some(Outcome::NotYet) => {
                // Zero declared reads as "as soon as possible", and as soon as
                // possible is the next invocation: taken literally it would
                // make the step ready in the loop that just postponed it, and
                // the executor would spin on one step for ever.
                let wait = step
                    .ask_again_after_secs
                    .map_or(NEXT_INVOCATION_AT_THE_EARLIEST, i64::from)
                    .max(NEXT_INVOCATION_AT_THE_EARLIEST);
                ready_or_later(latest.map(|record| ready_again_at(record, now, wait)));
            }
            Some(Outcome::Broke) if times_broken(step, records) >= step.max_attempts => {
                failed.push(step.id.clone());
            }
            Some(Outcome::Broke) => {
                ready_or_later(step.retry_after_secs.and_then(|wait| {
                    latest.map(|record| ready_again_at(record, now, i64::from(wait)))
                }));
            }
            None => ready_or_later(None),
        }
    }
    if !failed.is_empty() {
        Ok(Decision::Failed(failed))
    } else if !ready.is_empty() {
        Ok(Decision::Ready(ready))
    } else if !running.is_empty() {
        Ok(Decision::Running(running))
    } else if !not_yet.is_empty() {
        // Before `Waiting` on purpose: a run with something coming back is not
        // a run parked on a person, and reading it as one would send whoever
        // looks to go and take a step nobody handed them.
        let due_at = not_yet.iter().map(|(_, due)| *due).min().unwrap_or(now);
        Ok(Decision::NotYet {
            steps: not_yet.into_iter().map(|(step, _)| step).collect(),
            due_at,
        })
    } else if !waiting.is_empty() {
        Ok(Decision::Waiting(waiting))
    } else if !stopped.is_empty() {
        Ok(Decision::Stopped(stopped))
    } else {
        Ok(Decision::Complete)
    }
}

fn dependencies_satisfied(graph: &Graph, step: &Step, records: &[StepRecord]) -> bool {
    step.deps.iter().all(|dependency| {
        let outcome = records
            .iter()
            .filter(|record| record.step_id == *dependency)
            .max_by_key(|record| (record.attempt, record.epoch))
            .and_then(|record| record.outcome);
        outcome == Some(Outcome::Went)
            || (outcome == Some(Outcome::Skipped)
                && graph.dependency_is_skippable(&step.id, dependency))
    })
}

#[cfg(test)]
mod workdir_tests {
    use super::*;
    use crate::schema::ValueSchema;

    fn step_named(id: &str, with: Value, schema: ValueSchema) -> Step {
        let json = serde_json::json!({
            "id": id, "deps": [], "action": "whatever", "max_attempts": 1,
            "when": null,
            "input_schema": schema,
            "output_schema": {"type": "any"},
            "with": with
        });
        serde_json::from_value(json).expect("a valid step")
    }

    fn open_object() -> ValueSchema {
        serde_json::from_value(serde_json::json!({
            "type": "object", "properties": {}, "required": [], "allow_extra": true
        }))
        .expect("open schema")
    }

    fn resolved(with: Value, root: Option<&str>) -> Result<Value, FlowError> {
        let step = step_named("step", with, open_object());
        let input = step.with.clone().expect("the with is there");
        resolve_workdir(&step, input, root.map(Path::new))
    }

    /// A relative path hangs off the root, and that is the whole point: the
    /// same flow works in two different clones without changing a line.
    #[test]
    fn a_relative_workdir_hangs_off_the_root() {
        let out = resolved(serde_json::json!({"workdir": "crates/flow"}), Some("/here"))
            .expect("it resolves");

        assert_eq!(out["workdir"], "/here/crates/flow");
    }

    /// Absolute: an error naming the step and the value. Not "it runs
    /// elsewhere" — it runs in the *wrong* place, and that is how fault 25 went
    /// unnoticed: nothing failed, so nothing said anything.
    #[test]
    fn an_absolute_workdir_is_refused_by_name() {
        let refused = resolved(
            serde_json::json!({"workdir": "/work/sailor"}),
            Some("/here"),
        )
        .expect_err("it must not resolve");

        match refused {
            FlowError::AbsolutePath { step, value, .. } => {
                assert_eq!(step, "step");
                assert_eq!(value, "/work/sailor");
            }
            other => panic!("wrong error: {other}"),
        }
    }

    /// Absent: it inherits the root. That is what made it possible to take the
    /// seven `workdir` fields out of the development flow without any of its
    /// steps changing place.
    #[test]
    fn an_absent_workdir_inherits_the_root() {
        let out =
            resolved(serde_json::json!({"command": "true"}), Some("/here")).expect("it resolves");

        assert_eq!(out["workdir"], "/here");
    }

    /// But only to a step that can receive it. The trigger step of
    /// `sviluppa-sailor` has a closed schema and nothing whatever to do with a
    /// directory: offering it one would kill it on a field it never asked for.
    #[test]
    fn a_closed_schema_is_not_given_a_workdir_it_never_asked_for() {
        let closed: ValueSchema = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {"source": {"type": "string"}},
            "required": [],
            "allow_extra": false
        }))
        .expect("closed schema");
        let step = step_named("trigger", serde_json::json!({"source": "manual"}), closed);
        let input = step.with.clone().expect("the with is there");

        let out = resolve_workdir(&step, input, Some(Path::new("/here"))).expect("it resolves");

        assert!(out.get("workdir").is_none(), "no unrequested fields");
        step.input_schema.validate(&out).expect("the schema holds");
    }

    /// With no root it fails out loud, and never onto the `cwd`: a silent
    /// fallback to the process's own directory is exactly fault 25 — working
    /// wherever it lands, with nobody seeing it written anywhere.
    #[test]
    fn a_relative_workdir_without_a_root_fails_out_loud() {
        let refused = resolved(serde_json::json!({"workdir": "crates/flow"}), None)
            .expect_err("it must not fall back to the cwd");

        match refused {
            FlowError::NoWorkspaceRoot { step, value, .. } => {
                assert_eq!(step, "step");
                assert_eq!(value, "crates/flow");
            }
            other => panic!("wrong error: {other}"),
        }
        assert!(
            refused_says_how_to_fix(&resolved(
                serde_json::json!({"workdir": "crates/flow"}),
                None
            )),
            "the message must say what to do"
        );
    }

    fn refused_says_how_to_fix(outcome: &Result<Value, FlowError>) -> bool {
        match outcome {
            Err(error) => error.to_string().contains(crate::workspace::MARKER),
            Ok(_) => false,
        }
    }
}

/// Where a step works. Resolved here and not inside each action, so an action
/// nobody has written yet inherits it: fault 28 in the log.
pub const WORKDIR_FIELD: &str = "workdir";

/// Asks for a place of its own: the root is handed out here, so is the exception.
pub const TREE_FIELD: &str = "tree";

/// A step's input, and *whether that step runs*.
///
/// The two come back together because the condition decides whether references
/// resolve, and the resolved references are the input. Computing them in two
/// places would mean evaluating `when` twice, on two values that can differ by
/// exactly that resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInput {
    /// What the action receives: references resolved if the step runs, exactly
    /// as composed if the step is skipped.
    pub value: Value,
    /// False when the step's `when` is not satisfied.
    pub runs: bool,
}

/// How a step's input is composed, and the order is everything: dependency
/// output with `with` over it, then the `workdir` — which depends on neither of
/// the two below it — then the condition, and last the references, only if the
/// step runs. Composing them in two places would evaluate `when` twice on two
/// values that differ by exactly that resolution: the same rule written twice,
/// which is fault 10 inside the cure for fault 28.
pub fn step_input(
    graph: &Graph,
    step: &Step,
    root_inputs: &BTreeMap<String, Value>,
    records: &[StepRecord],
    root: Option<&Path>,
) -> Result<StepInput, FlowError> {
    let composed = composed_input(graph, step, root_inputs, records)?;
    let positioned = resolve_workdir(step, composed, root)?;
    // The condition is judged on the input not yet resolved, and this was
    // measured: `flows/chiedi-all-indice.flow.json` has step `leggi` with a
    // `when` on `/status` and a `with` full of `$from` into the output of
    // `chiedi`, a skippable dependency. Resolving first took that flow, on the
    // real binary, from "complete" to "failed — `unresolved_reference`". A step
    // that will not run must not pay for references to work it will not do.
    let runs = step
        .when
        .as_ref()
        .is_none_or(|condition| condition.matches(&positioned));
    if !runs {
        return Ok(StepInput {
            value: positioned,
            runs,
        });
    }
    let value = match step.with.as_ref() {
        Some(with) => {
            let resolved =
                reference::resolve_overlay(with, &positioned).map_err(FlowError::Action)?;
            overlay_input(positioned, Some(&resolved))
        }
        None => positioned,
    };
    Ok(StepInput { value, runs })
}

/// The input *as the step receives it*: the dependencies' output with `with`
/// laid over, and nothing else — no reference resolved, no path resolved.
///
/// It stands apart because it is needed twice: to compose, and to *report* a
/// step that broke while resolving references. Whoever reads that record must
/// see the pointer they wrote, not the hole it left.
fn composed_input(
    graph: &Graph,
    step: &Step,
    root_inputs: &BTreeMap<String, Value>,
    records: &[StepRecord],
) -> Result<Value, FlowError> {
    let input = match step.deps.as_slice() {
        [] => Ok(root_inputs.get(&step.id).cloned().unwrap_or(Value::Null)),
        [only] if !graph.dependency_is_skippable(&step.id, only) => {
            successful_output(only, records)
        }
        many => {
            let mut values = serde_json::Map::new();
            for dependency in many {
                if let Some(output) = dependency_output(
                    dependency,
                    graph.dependency_is_skippable(&step.id, dependency),
                    records,
                )? {
                    values.insert(dependency.clone(), output);
                }
            }
            Ok(Value::Object(values))
        }
    }?;
    Ok(overlay_input(input, step.with.as_ref()))
}

/// Where the step will work, decided here and not inside the action. Four
/// cases, not one a silent fallback: absolute → an error naming step and value,
/// since such a flow runs in one place and elsewhere would not fail but work in
/// the wrong one; relative → hung off the root; absent → the root, but only to
/// a step that can receive it (`accepts_property`); no root where one is needed
/// → a readable error, never the `cwd`, which would be fault 25.
fn resolve_workdir(step: &Step, input: Value, root: Option<&Path>) -> Result<Value, FlowError> {
    let Value::Object(mut fields) = input else {
        return Ok(input);
    };
    match fields.get(WORKDIR_FIELD).cloned() {
        Some(Value::String(declared)) => {
            if declared.starts_with('/') || declared.starts_with("~/") {
                return Err(FlowError::AbsolutePath {
                    step: step.id.clone(),
                    field: WORKDIR_FIELD.to_owned(),
                    value: declared,
                });
            }
            let Some(root) = root else {
                return Err(FlowError::NoWorkspaceRoot {
                    step: step.id.clone(),
                    field: WORKDIR_FIELD.to_owned(),
                    value: declared,
                });
            };
            fields.insert(
                WORKDIR_FIELD.to_owned(),
                root.join(declared).display().to_string().into(),
            );
        }
        // Declared but not as text: it is not a path, and inventing one would
        // be worse than passing it on to whoever knows what to do with it. So a
        // `{"$from": …}` workdir is never hung off the root — it resolves after.
        Some(_) => {}
        None => {
            // Its own tree means no shared root: it would arrive as a `workdir`
            // its author never wrote.
            let has_its_own = fields.contains_key(TREE_FIELD);
            if let Some(root) = root
                .filter(|_| !has_its_own && step.input_schema.accepts_property(WORKDIR_FIELD))
            {
                fields.insert(WORKDIR_FIELD.to_owned(), root.display().to_string().into());
            }
        }
    }
    Ok(Value::Object(fields))
}

fn overlay_input(input: Value, with: Option<&Value>) -> Value {
    let Some(with) = with else {
        return input;
    };
    let Value::Object(with) = with else {
        return with.clone();
    };
    let Value::Object(mut input) = input else {
        return Value::Object(with.clone());
    };
    input.extend(with.clone());
    Value::Object(input)
}

pub fn attempt_relation(records: &[StepRecord], started: &StepRecord) -> Option<AttemptRelation> {
    let previous = records
        .iter()
        .filter(|record| {
            record.step_id == started.step_id
                && (record.attempt < started.attempt || record.epoch < started.epoch)
        })
        .max_by_key(|record| (record.attempt, record.epoch))?;
    if previous.input_digest != started.input_digest {
        Some(AttemptRelation::DifferentInput)
    } else {
        let origin = records
            .iter()
            .filter(|record| {
                record.step_id == started.step_id && record.input_digest == started.input_digest
            })
            .min_by_key(|record| (record.attempt, record.epoch))
            .unwrap_or(previous);
        if same_gates(&origin.gates, &started.gates) {
            Some(AttemptRelation::SameInput)
        } else {
            Some(AttemptRelation::SameInputGatesChanged)
        }
    }
}

pub fn latest_for<'a>(step: &Step, records: &'a [StepRecord]) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step.id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

pub fn same_gates(left: &[String], right: &[String]) -> bool {
    let left: std::collections::BTreeSet<_> = left.iter().collect();
    let right: std::collections::BTreeSet<_> = right.iter().collect();
    left == right
}

fn successful_output(step_id: &str, records: &[StepRecord]) -> Result<Value, FlowError> {
    records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went))
        .max_by_key(|record| (record.attempt, record.epoch))
        .and_then(|record| record.output.clone())
        .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned()))
}

fn dependency_output(
    step_id: &str,
    skippable: bool,
    records: &[StepRecord],
) -> Result<Option<Value>, FlowError> {
    let latest = records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch));
    match latest.and_then(|record| record.outcome) {
        Some(Outcome::Went) => latest
            .and_then(|record| record.output.clone())
            .map(Some)
            .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned())),
        Some(Outcome::Skipped) if skippable => Ok(None),
        _ => Err(FlowError::MissingOutput(step_id.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Condition, ValueSchema};
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A fake clock advancing by one per question, on an atomic counter: the
    /// threads of a front now share the clock.
    struct Tick(std::sync::atomic::AtomicI64);

    impl Tick {
        fn new(start: i64) -> Self {
            Tick(std::sync::atomic::AtomicI64::new(start))
        }
    }

    impl Clock for Tick {
        fn now(&self) -> Result<i64, FlowError> {
            Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
        }
    }

    struct Echo;

    impl Action for Echo {
        fn execute(
            &self,
            input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Went(input.clone()))
        }
    }

    struct FailOnce(Arc<AtomicUsize>);

    impl Action for FailOnce {
        fn execute(
            &self,
            input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(ActionError::new("temporary", "try again"))
            } else {
                Ok(ActionOutcome::Went(input.clone()))
            }
        }
    }

    struct Wait;

    impl Action for Wait {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Waiting("source unreadable".to_owned()))
        }
    }

    struct Empty;

    impl Action for Empty {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Went(json!([])))
        }
    }

    fn step(id: &str, deps: &[&str], action: &str, max_attempts: u32) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|id| (*id).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::Any,
            with: None,
            when: None,
            action: action.to_owned(),
            max_attempts,
            ask_again_after_secs: None,
            retry_after_secs: None,
            phase: None,
        }
    }

    /// The executor tells the action which step's work it is doing, and tells
    /// it at every step: without that, an action producing text as it runs
    /// could not attribute it, and in a graph with two live steps a reader
    /// would see two nameless entries mixed together.
    #[test]
    fn each_action_sees_the_id_of_the_step_it_is_running() {
        struct WhoAmI(Arc<std::sync::Mutex<Vec<String>>>);

        impl Action for WhoAmI {
            fn execute(
                &self,
                _input: &Value,
                shared: &SharedState,
            ) -> Result<ActionOutcome, ActionError> {
                let seen = shared
                    .get(CURRENT_STEP)
                    .and_then(Value::as_str)
                    .unwrap_or("nobody")
                    .to_owned();
                self.0.lock().expect("nobody panics here").push(seen);
                Ok(ActionOutcome::Went(json!({})))
            }
        }

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let graph = Graph::new(vec![
            step("first", &[], "who-am-i", 1),
            step("second", &["first"], "who-am-i", 1),
        ])
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("who-am-i", WhoAmI(seen.clone()));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: BTreeMap::new(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("esecuzione riuscita");
        assert_eq!(
            *seen.lock().expect("nobody panics here"),
            vec!["first".to_owned(), "second".to_owned()]
        );
    }

    /// A check that refuses leaves the row saying which check and what it saw:
    /// as a record beside the class, for a count, and as the opening of the
    /// sentence a person reads. The step's own output schema is one such check.
    #[test]
    fn a_refused_step_records_which_check_refused_and_opens_its_sentence_with_it() {
        struct Refusing;

        impl Action for Refusing {
            fn execute(
                &self,
                _input: &Value,
                _shared: &SharedState,
            ) -> Result<ActionOutcome, ActionError> {
                Err(ActionError::new("answer_off_shape", "off shape").refused(Refusal::new(
                    "answer_shape",
                    "$.verdict",
                    crate::RefusalRule::NotAllowed,
                    "\"remvoe\"",
                )))
            }
        }

        let mut off_shape = step("shaped", &[], "refusing", 1);
        off_shape.output_schema = ValueSchema::Any;
        let mut typed = step("typed", &[], "echo", 1);
        typed.output_schema = ValueSchema::Number;
        let graph = Graph::new(vec![off_shape, typed]).expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("refusing", Refusing);
        actions.register("echo", Echo);
        let mut roots = BTreeMap::new();
        roots.insert("typed".to_owned(), json!("not a number"));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: roots,
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        let _ = InProcessExecutor.execute(&graph, request, &store, &actions, &Tick::new(0));

        let records = store.records("run").expect("records");
        let shaped = records
            .iter()
            .find(|record| record.step_id == "shaped")
            .expect("the shaped step ran");
        let refusal = shaped.refusal.as_ref().expect("the refusal is recorded");
        assert_eq!(refusal.check, "answer_shape");
        assert_eq!(refusal.seen, "\"remvoe\"");
        let said = shaped.said.as_deref().expect("the step said why");
        assert!(said.starts_with(&refusal.explain()), "{said}");
        assert!(said.ends_with("off shape"), "{said}");

        let typed = records
            .iter()
            .find(|record| record.step_id == "typed")
            .expect("the typed step ran");
        assert_eq!(typed.failure_class.as_deref(), Some("invalid_output"));
        let refusal = typed.refusal.as_ref().expect("the output schema is a check");
        assert_eq!(refusal.check, "output_schema");
        assert_eq!(refusal.rule, crate::RefusalRule::WrongType);
        assert_eq!(refusal.seen, "\"not a number\"");
    }

    /// What an action reports it ran reaches the closed record, on the step
    /// that went and on the one that broke alike: whoever reads the run later
    /// must see what was executed, not only how it ended.
    #[test]
    fn the_record_keeps_the_line_the_action_ran_whether_it_went_or_broke() {
        struct Running;

        impl Action for Running {
            fn execute(
                &self,
                _input: &Value,
                _shared: &SharedState,
            ) -> Result<ActionOutcome, ActionError> {
                Ok(ActionOutcome::Went(json!({"status": "passed"})))
            }

            fn execute_and_report(
                &self,
                input: &Value,
                shared: &SharedState,
            ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
                let ran = Ran::new("sh", ["-c", "true"]);
                if input.get("break").is_some() {
                    return Err(ActionError::new("check_failed", "exit 2").having_run(ran));
                }
                self.execute(input, shared).map(|outcome| (outcome, Some(ran)))
            }
        }

        let graph = Graph::new(vec![
            step("went", &[], "running", 1),
            step("broke", &[], "running", 1),
        ])
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("running", Running);
        let mut roots = BTreeMap::new();
        roots.insert("broke".to_owned(), json!({"break": true}));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: roots,
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        let _ = InProcessExecutor.execute(&graph, request, &store, &actions, &Tick::new(0));

        let records = store.records("run").expect("records");
        let expected = Some(Ran::new("sh", ["-c", "true"]));
        let went = records
            .iter()
            .find(|record| record.step_id == "went")
            .expect("the step that went ran");
        assert_eq!(went.outcome, Some(Outcome::Went));
        assert_eq!(went.ran, expected, "the line was dropped on the way to the record");
        let broke = records
            .iter()
            .find(|record| record.step_id == "broke")
            .expect("the step that broke ran");
        assert_eq!(broke.outcome, Some(Outcome::Broke));
        assert_eq!(broke.ran, expected, "a broken step forgot the line it ran");
    }

    /// A store that answers "stop" once a number of fronts have asked.
    struct HaltAfter {
        inner: InMemoryRecordStore,
        fronts: usize,
        asked: AtomicUsize,
    }

    impl RecordStore for HaltAfter {
        fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
            self.inner.append_started(record)
        }
        fn close(
            &self,
            run_id: &str,
            step_id: &str,
            attempt: u32,
            epoch: u64,
            completion: Completion,
        ) -> Result<(), FlowError> {
            self.inner.close(run_id, step_id, attempt, epoch, completion)
        }
        fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
            self.inner.records(run_id)
        }
        fn spent(&self, run_id: &str) -> Result<Spend, FlowError> {
            self.inner.spent(run_id)
        }
        fn halt_requested(&self, _run_id: &str) -> Result<bool, FlowError> {
            Ok(self.asked.fetch_add(1, Ordering::SeqCst) >= self.fronts)
        }
    }

    /// **A STOP ASKED BY HAND ENDS THE RUN BEFORE THE NEXT FRONT, AND SAYS
    /// WHICH STEPS IT LEFT.** The first front runs to its end; the second is
    /// handed back as not started, and the run's word is `stopped`, not
    /// `failed`: nothing broke. Without the check in the loop the second step
    /// would run and this would go red on the records.
    #[test]
    fn a_stop_asked_by_hand_holds_the_next_front_and_names_it() {
        let graph = Graph::new(vec![
            step("first", &[], "echo", 1),
            step("second", &["first"], "echo", 1),
        ])
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: BTreeMap::new(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = HaltAfter {
            inner: InMemoryRecordStore::default(),
            fronts: 1,
            asked: AtomicUsize::new(0),
        };
        let result = InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("the run ends without an error");
        assert_eq!(
            result.decisions,
            vec![
                Decision::Ready(vec!["first".to_owned()]),
                Decision::Ready(vec!["second".to_owned()]),
                Decision::Halted(vec!["second".to_owned()]),
            ]
        );
        assert_eq!(run_status(&result), ("stopped", false));
        let opened: Vec<String> = store.inner.all().into_iter().map(|record| record.step_id).collect();
        assert_eq!(opened, vec!["first".to_owned()], "the second step must not have opened");
    }

    #[test]
    fn branch_and_join_use_ready_fronts_and_typed_values() {
        let graph = Graph::new(vec![
            step("root", &[], "echo", 1),
            step("left", &["root"], "echo", 1),
            step("right", &["root"], "echo", 1),
            step("join", &["left", "right"], "echo", 1),
        ])
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("root".to_owned(), json!({"value": "said is not data"}))]
                .into_iter()
                .collect(),
            gates: vec!["filesystem".to_owned()],
            shared: [("budget".to_owned(), json!(10))].into_iter().collect(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        let result = InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("esecuzione riuscita");
        assert_eq!(
            result.decisions,
            vec![
                Decision::Ready(vec!["root".to_owned()]),
                Decision::Ready(vec!["left".to_owned(), "right".to_owned()]),
                Decision::Ready(vec!["join".to_owned()]),
                Decision::Complete,
            ]
        );
        let records = store.all();
        let join = records
            .iter()
            .find(|record| record.step_id == "join")
            .expect("the join's record");
        assert_eq!(join.input["left"]["value"], "said is not data");
        assert_eq!(join.input["right"]["value"], "said is not data");
    }

    #[test]
    fn dependent_step_merges_its_values_over_predecessor_output() {
        let mut send = step("send", &["panel"], "echo", 1);
        send.with = Some(json!({"text": "/clear", "mode": "declared"}));
        let graph = Graph::new(vec![step("panel", &[], "echo", 1), send]).expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [(
                "panel".to_owned(),
                json!({"panel": "p-7", "mode": "predecessor"}),
            )]
            .into_iter()
            .collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();

        InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("esecuzione riuscita");

        let records = store.all();
        let send = records
            .iter()
            .find(|record| record.step_id == "send")
            .expect("record dell'invio");
        assert_eq!(
            send.input,
            json!({"panel": "p-7", "mode": "declared", "text": "/clear"})
        );
    }

    #[test]
    fn action_can_wait_without_failure_or_retry() {
        let graph = Graph::new(vec![
            step("uncertain", &[], "wait", 3),
            step("later", &["uncertain"], "echo", 1),
        ])
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("wait", Wait);
        actions.register("echo", Echo);
        let store = InMemoryRecordStore::default();
        let execution = InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: BTreeMap::new(),
                    gates: vec![],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &store,
                &actions,
                &Tick::new(0),
            )
            .expect("waiting is a legitimate outcome");

        assert_eq!(
            execution.decisions,
            vec![
                Decision::Ready(vec!["uncertain".to_owned()]),
                Decision::Waiting(vec!["uncertain".to_owned()]),
            ]
        );
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].outcome, Some(Outcome::Waiting));
        assert_eq!(store.all()[0].failure_class, None);
    }

    #[test]
    fn conditional_join_omits_skipped_input_but_keeps_present_empty_input() {
        let mut skipped = step("skipped", &["root"], "empty", 1);
        skipped.when = Some(Condition::PointerEquals {
            pointer: "/take_skipped".to_owned(),
            value: json!(true),
        });
        let graph = Graph::with_skippable_dependencies(
            vec![
                step("root", &[], "echo", 1),
                skipped,
                step("present_empty", &["root"], "empty", 1),
                step("join", &["skipped", "present_empty"], "echo", 1),
            ],
            [crate::DependencyEdge::new("join", "skipped")],
        )
        .expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        actions.register("empty", Empty);
        let store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: [("root".to_owned(), json!({"take_skipped": false}))]
                        .into_iter()
                        .collect(),
                    gates: vec![],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &store,
                &actions,
                &Tick::new(0),
            )
            .expect("la giunzione parte");

        let records = store.all();
        let join = records
            .iter()
            .find(|record| record.step_id == "join")
            .expect("the join's record");
        let input = join.input.as_object().expect("composed input");
        assert!(!input.contains_key("skipped"));
        assert_eq!(input.get("present_empty"), Some(&json!([])));
        assert_eq!(join.outcome, Some(Outcome::Went));
    }

    /// **A `$from` INSIDE A DEPENDENCY'S OUTPUT IS DATA.** A flow a model had
    /// drafted, carrying its own references for a run yet to happen, broke the
    /// step that was to write it: the resolver hunted the whole input. Only the
    /// `with` is written by whoever writes the flow; only the `with` is read.
    #[test]
    fn a_reference_inside_a_dependencys_output_is_left_as_it_is() {
        let mut next = step("next", &["root"], "echo", 1);
        next.with = Some(json!({ "copy": { "$from": "/text" } }));
        let graph = Graph::new(vec![step("root", &[], "echo", 1), next]).expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: [(
                        "root".to_owned(),
                        json!({ "text": "hello", "drafted": { "$from": "/nowhere" } }),
                    )]
                    .into_iter()
                    .collect(),
                    gates: vec![],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &store,
                &actions,
                &Tick::new(0),
            )
            .expect("the run goes");

        let records = store.all();
        let next = records
            .iter()
            .find(|record| record.step_id == "next")
            .expect("the next step's record");
        assert_eq!(next.outcome, Some(Outcome::Went), "{:?}", next.said);
        assert_eq!(next.input["copy"], json!("hello"), "the with's own reference is resolved");
        assert_eq!(
            next.input["drafted"],
            json!({ "$from": "/nowhere" }),
            "the dependency's data arrives as it was"
        );
    }

    #[test]
    fn retry_repeats_only_the_failed_step() {
        let graph = Graph::new(vec![
            step("first", &[], "echo", 1),
            step("retry", &["first"], "flaky", 2),
        ])
        .expect("valid graph");
        let count = Arc::new(AtomicUsize::new(0));
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        actions.register("flaky", FailOnce(count));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("first".to_owned(), json!("input"))].into_iter().collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("the second attempt succeeds");
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|record| record.step_id == "first")
                .count(),
            1
        );
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|record| record.step_id == "retry")
                .count(),
            2
        );
    }

    #[test]
    fn retry_with_same_input_and_changed_gates_is_explicit() {
        let graph = Graph::new(vec![step("work", &[], "echo", 2)]).expect("valid graph");
        let input = json!({"payload": 7});
        let mut first = StepRecord::started(
            "run",
            "work",
            1,
            1,
            vec![],
            input.clone(),
            vec!["filesystem".to_owned()],
            1,
        );
        first.outcome = Some(Outcome::Broke);
        first.failure_class = Some("temporary".to_owned());
        first.ended_at = Some(2);
        let store = InMemoryRecordStore::from_records(vec![first]);
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);

        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: [("work".to_owned(), input)].into_iter().collect(),
                    gates: vec!["network".to_owned(), "filesystem".to_owned()],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &store,
                &actions,
                &Tick::new(2),
            )
            .expect("ripresa riuscita");

        let attempts = store.all();
        assert_eq!(attempts[0].input_digest, attempts[1].input_digest);
        assert_eq!(
            attempts[1].attempt_relation,
            Some(AttemptRelation::SameInputGatesChanged)
        );
        assert_eq!(attempts[1].said, None);
    }

    #[test]
    fn later_epoch_fences_a_returning_attempt() {
        let store = InMemoryRecordStore::default();
        let mut first = StepRecord::started("run", "step", 1, 4, vec![], json!(null), vec![], 1);
        first.outcome = Some(Outcome::Broke);
        first.failure_class = Some("dead".to_owned());
        first.ended_at = Some(2);
        store.held().push(first);
        store
            .append_started(StepRecord::started(
                "run",
                "step",
                2,
                5,
                vec![],
                json!(null),
                vec![],
                3,
            ))
            .expect("epoca successiva");
        let result = store.close(
            "run",
            "step",
            1,
            4,
            Completion {
                outcome: Outcome::Went,
                output: Some(json!("late")),
                said: None,
                failure_class: None,
                refusal: None,
                ran: None,
                ended_at: 4,
                bytes_seen: None,
                bytes_discarded: None,
            },
        );
        assert_eq!(
            result,
            Err(FlowError::StaleEpoch {
                step: "step".to_owned(),
                epoch: 4
            })
        );
    }

    #[test]
    fn condition_reads_typed_input_and_never_said() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut conditional = step("conditional", &[], "action", 1);
        conditional.when = Some(Condition::PointerEquals {
            pointer: "/approved".to_owned(),
            value: json!(true),
        });
        let graph = Graph::new(vec![conditional]).expect("valid graph");
        let mut actions = ActionRegistry::default();
        actions.register("action", FailOnce(Arc::clone(&count)));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [(
                "conditional".to_owned(),
                json!({"approved": false, "said": "approved"}),
            )]
            .into_iter()
            .collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let store = InMemoryRecordStore::default();
        let execution = InProcessExecutor
            .execute(&graph, request, &store, &actions, &Tick::new(0))
            .expect("condizione valutata");
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(store.all()[0].outcome, Some(Outcome::Skipped));
        assert_eq!(
            execution.decisions,
            vec![
                Decision::Ready(vec!["conditional".to_owned()]),
                Decision::Complete,
            ]
        );
    }

    /// A stopped clock that remembers which threads asked it. Stopped, so that
    /// two readers getting the same answer means one clock and not luck.
    struct Overheard {
        at: i64,
        askers: Mutex<HashSet<std::thread::ThreadId>>,
    }

    impl Overheard {
        fn at(instant: i64) -> Self {
            Overheard {
                at: instant,
                askers: Mutex::new(HashSet::new()),
            }
        }

        fn askers(&self) -> HashSet<std::thread::ThreadId> {
            self.askers.lock().expect("nobody panics here").clone()
        }
    }

    impl Clock for Overheard {
        fn now(&self) -> Result<i64, FlowError> {
            self.askers
                .lock()
                .expect("nobody panics here")
                .insert(std::thread::current().id());
            Ok(self.at)
        }
    }

    /// How long a step waits for the companions it was told to expect.
    const LONG_ENOUGH: std::time::Duration = std::time::Duration::from_secs(5);

    /// A step that holds until `expected` others have entered, so they are all
    /// alive at once rather than one after the other.
    struct WaitsForTheOthers {
        arrived: Arc<AtomicUsize>,
        expected: usize,
        threads: Arc<Mutex<HashSet<std::thread::ThreadId>>>,
    }

    impl Action for WaitsForTheOthers {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            self.threads
                .lock()
                .expect("nobody panics here")
                .insert(std::thread::current().id());
            self.arrived.fetch_add(1, Ordering::SeqCst);
            let until = std::time::Instant::now() + LONG_ENOUGH;
            while self.arrived.load(Ordering::SeqCst) < self.expected {
                if std::time::Instant::now() >= until {
                    return Err(ActionError::new("on_its_own", "nobody else came"));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(ActionOutcome::Went(json!({})))
        }
    }

    /// A front of two steps holding each other, on the action `waits`.
    fn a_front_that_waits(
        expected: usize,
    ) -> (
        Graph,
        ActionRegistry,
        Arc<AtomicUsize>,
        Arc<Mutex<HashSet<std::thread::ThreadId>>>,
    ) {
        let arrived = Arc::new(AtomicUsize::new(0));
        let threads = Arc::new(Mutex::new(HashSet::new()));
        let mut actions = ActionRegistry::default();
        actions.register(
            "waits",
            WaitsForTheOthers {
                arrived: Arc::clone(&arrived),
                expected,
                threads: Arc::clone(&threads),
            },
        );
        let graph = Graph::new(vec![
            step("first", &[], "waits", 1),
            step("second", &[], "waits", 1),
        ])
        .expect("valid graph");
        (graph, actions, arrived, threads)
    }

    fn a_request(run_id: &str) -> ExecutionRequest {
        ExecutionRequest {
            run_id: run_id.to_owned(),
            root_inputs: BTreeMap::new(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        }
    }

    /// Two steps of one front, alive together, ask the same clock and each is
    /// answered on its own thread with the same instant.
    #[test]
    fn two_steps_of_one_front_read_the_same_clock() {
        let (graph, actions, _, threads) = a_front_that_waits(2);
        let store = InMemoryRecordStore::default();
        let clock = Overheard::at(1_000);

        InProcessExecutor
            .execute(&graph, a_request("run"), &store, &actions, &clock)
            .expect("the execution reaches the end");

        let ran_on = threads.lock().expect("nobody panics here").clone();
        assert_eq!(ran_on.len(), 2, "the front ran on two threads");
        assert!(
            ran_on.is_subset(&clock.askers()),
            "the one clock answered on the thread of each step"
        );
        for record in store.all() {
            assert_eq!(
                record.outcome,
                Some(Outcome::Went),
                "{} waited for a companion that never came",
                record.step_id
            );
            assert_eq!(record.started_at, 1_000);
            assert_eq!(record.ended_at, Some(1_000));
        }
    }

    /// A run at work and a reconciliation hold one clock at the same moment,
    /// and both are answered from the same instant.
    ///
    /// The request took the clock exclusively, and an exclusive borrow admits
    /// no second holder: give it back and this stops compiling.
    #[test]
    fn a_run_and_a_reconciliation_hold_one_clock_at_once() {
        struct NothingRuns;

        impl ProcessProbe for NothingRuns {
            fn is_running(&self, _record: &StepRecord) -> Result<bool, FlowError> {
                Ok(false)
            }
        }

        let (graph, actions, arrived, threads) = a_front_that_waits(3);
        let store = InMemoryRecordStore::default();
        let mut abandoned = InMemoryRecordStore::default();
        abandoned
            .append_started(StepRecord::started(
                "left-behind",
                "first",
                1,
                1,
                vec![],
                json!({}),
                vec![],
                1,
            ))
            .expect("the abandoned run is written");
        let clock = Overheard::at(1_000);
        let shared = SharedState::new();

        let report = std::thread::scope(|scope| {
            let running = scope.spawn(|| {
                InProcessExecutor.execute(&graph, a_request("run"), &store, &actions, &clock)
            });
            let until = std::time::Instant::now() + LONG_ENOUGH;
            while arrived.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let report = InProcessExecutor
                .reconcile(ReconciliationRequest {
                    graph: &graph,
                    run_id: "left-behind",
                    store: &mut abandoned,
                    actions: &actions,
                    shared: &shared,
                    processes: &NothingRuns,
                    clock: &clock,
                })
                .expect("the reconciliation answers");
            arrived.fetch_add(1, Ordering::SeqCst);
            running
                .join()
                .expect("the run does not panic")
                .expect("the execution reaches the end");
            report
        });

        assert_eq!(report.closed_as_waiting, vec!["first".to_owned()]);
        assert_eq!(
            abandoned.all()[0].ended_at,
            Some(1_000),
            "the reconciliation read the instant the run reads"
        );
        let ran_on = threads.lock().expect("nobody panics here").clone();
        assert_eq!(ran_on.len(), 2, "the front ran on two threads");
        let askers = clock.askers();
        assert!(
            ran_on.is_subset(&askers),
            "the one clock answered on the thread of each step"
        );
        assert!(
            askers.contains(&std::thread::current().id()),
            "and on the thread that reconciled while they ran"
        );
    }
}
