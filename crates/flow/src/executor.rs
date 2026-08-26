use crate::record::truncate_said;
use crate::{Graph, Outcome, SchemaError, Step, StepRecord};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

pub type SharedState = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError {
    pub class: String,
    pub said: String,
}

impl ActionError {
    pub fn new(class: impl Into<String>, said: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            said: said.into(),
        }
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

pub trait Action: Send + Sync {
    fn execute(&self, input: &Value, shared: &mut SharedState) -> Result<Value, ActionError>;

    /// Un'azione senza una prova positiva non è rilanciabile automaticamente:
    /// `Unknown` conserva l'ambiguità invece di duplicare un effetto esterno.
    fn inspect_effect(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        Ok(EffectStatus::Unknown("effect_not_inspectable".to_owned()))
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub outcome: Outcome,
    pub output: Option<Value>,
    pub said: Option<String>,
    pub failure_class: Option<String>,
    pub ended_at: i64,
}

pub trait RecordStore {
    /// Deve rendere durevole l'intenzione prima di restituire al chiamante.
    fn append_started(&mut self, record: StepRecord) -> Result<(), FlowError>;
    fn close(
        &mut self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError>;
    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError>;
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryRecordStore {
    records: Vec<StepRecord>,
}

impl InMemoryRecordStore {
    pub fn from_records(records: Vec<StepRecord>) -> Self {
        Self { records }
    }

    pub fn all(&self) -> &[StepRecord] {
        &self.records
    }
}

impl RecordStore for InMemoryRecordStore {
    fn append_started(&mut self, record: StepRecord) -> Result<(), FlowError> {
        if record.outcome.is_some()
            || record.output.is_some()
            || record.said.is_some()
            || record.failure_class.is_some()
            || record.ended_at.is_some()
        {
            return Err(FlowError::InvalidRecord(
                "a started record already contains closing fields".to_owned(),
            ));
        }
        let duplicate = self.records.iter().any(|found| {
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
        let greatest_epoch = self
            .records
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
        self.records.push(record);
        Ok(())
    }

    fn close(
        &mut self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        mut completion: Completion,
    ) -> Result<(), FlowError> {
        let greatest_epoch = self
            .records
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
        let record = self
            .records
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
        record.ended_at = Some(completion.ended_at);
        Ok(())
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        Ok(self
            .records
            .iter()
            .filter(|record| record.run_id == run_id)
            .cloned()
            .collect())
    }
}

pub trait Clock {
    fn now(&mut self) -> Result<i64, FlowError>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Result<i64, FlowError> {
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
    Stopped(Vec<String>),
    Failed(Vec<String>),
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub decisions: Vec<Decision>,
    pub shared: SharedState,
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub run_id: String,
    pub root_inputs: BTreeMap<String, Value>,
    pub gates: Vec<String>,
    pub shared: SharedState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reconciliation {
    pub closed_as_went: Vec<String>,
    pub closed_as_broke: Vec<String>,
    pub closed_as_waiting: Vec<String>,
    pub still_running: Vec<String>,
}

pub struct ReconciliationRequest<'a> {
    pub graph: &'a Graph,
    pub run_id: &'a str,
    pub store: &'a mut dyn RecordStore,
    pub actions: &'a ActionRegistry,
    pub shared: &'a SharedState,
    pub processes: &'a dyn ProcessProbe,
    pub clock: &'a mut dyn Clock,
}

pub trait Executor {
    fn execute(
        &self,
        graph: &Graph,
        request: ExecutionRequest,
        store: &mut dyn RecordStore,
        actions: &ActionRegistry,
        clock: &mut dyn Clock,
    ) -> Result<Execution, FlowError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessExecutor;

impl InProcessExecutor {
    pub fn decision(
        &self,
        graph: &Graph,
        run_id: &str,
        store: &dyn RecordStore,
    ) -> Result<Decision, FlowError> {
        let records = store.records(run_id)?;
        decision_from(graph, &records)
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
            let inspected = graph.step(&record.step_id).map_or_else(
                || Err(ActionError::new("unknown_step", &record.step_id)),
                |step| {
                    actions.get(&step.action).map_or_else(
                        || Err(ActionError::new("unknown_action", &step.action)),
                        |action| {
                            action.inspect_effect(record, shared).and_then(|status| {
                                if let EffectStatus::Applied(output) = &status {
                                    step.output_schema.validate(output).map_err(|error| {
                                        ActionError::new(
                                            "invalid_recovered_output",
                                            error.to_string(),
                                        )
                                    })?;
                                }
                                Ok(status)
                            })
                        },
                    )
                },
            );
            let (completion, bucket) = match inspected {
                Ok(EffectStatus::Applied(output)) => (
                    Completion {
                        outcome: Outcome::Went,
                        output: Some(output),
                        said: None,
                        failure_class: None,
                        ended_at: clock.now()?,
                    },
                    &mut report.closed_as_went,
                ),
                Ok(EffectStatus::NotApplied) => (
                    Completion {
                        outcome: Outcome::Broke,
                        output: None,
                        said: None,
                        failure_class: Some("process_disappeared".to_owned()),
                        ended_at: clock.now()?,
                    },
                    &mut report.closed_as_broke,
                ),
                Ok(EffectStatus::Unknown(reason)) => (
                    Completion {
                        outcome: Outcome::Waiting,
                        output: None,
                        said: Some(reason),
                        failure_class: Some("effect_unknown".to_owned()),
                        ended_at: clock.now()?,
                    },
                    &mut report.closed_as_waiting,
                ),
                Err(error) => (
                    Completion {
                        outcome: Outcome::Waiting,
                        output: None,
                        said: Some(error.said),
                        failure_class: Some(error.class),
                        ended_at: clock.now()?,
                    },
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

    fn input_for(
        &self,
        step: &Step,
        root_inputs: &BTreeMap<String, Value>,
        records: &[StepRecord],
    ) -> Result<Value, FlowError> {
        match step.deps.as_slice() {
            [] => Ok(root_inputs.get(&step.id).cloned().unwrap_or(Value::Null)),
            [only] => successful_output(only, records),
            many => {
                let mut values = serde_json::Map::new();
                for dependency in many {
                    values.insert(dependency.clone(), successful_output(dependency, records)?);
                }
                Ok(Value::Object(values))
            }
        }
    }
}

impl Executor for InProcessExecutor {
    fn execute(
        &self,
        graph: &Graph,
        mut request: ExecutionRequest,
        store: &mut dyn RecordStore,
        actions: &ActionRegistry,
        clock: &mut dyn Clock,
    ) -> Result<Execution, FlowError> {
        let mut decisions = Vec::new();
        loop {
            let records = store.records(&request.run_id)?;
            let decision = decision_from(graph, &records)?;
            decisions.push(decision.clone());
            let Decision::Ready(front) = decision else {
                return Ok(Execution {
                    decisions,
                    shared: request.shared,
                });
            };

            // Il fronte è una decisione unica anche se questo esecutore lo percorre
            // in ordine: l'esecutore di processi potrà avviarlo in parallelo.
            for step_id in front {
                let step = graph
                    .step(&step_id)
                    .ok_or_else(|| FlowError::UnknownStep(step_id.clone()))?;
                let input = self.input_for(step, &request.root_inputs, &records)?;
                step.input_schema.validate(&input)?;
                let condition_met = step
                    .when
                    .as_ref()
                    .is_none_or(|condition| condition.matches(&input));
                let action = if condition_met {
                    Some(
                        actions
                            .get(&step.action)
                            .ok_or_else(|| FlowError::UnknownAction(step.action.clone()))?,
                    )
                } else {
                    None
                };
                let previous = latest_for(step, &records);
                let attempt = previous.map_or(1, |record| record.attempt + 1);
                let epoch = records.iter().map(|record| record.epoch).max().unwrap_or(0) + 1;
                let started = StepRecord::started(
                    &request.run_id,
                    &step.id,
                    attempt,
                    epoch,
                    step.deps.clone(),
                    input.clone(),
                    request.gates.clone(),
                    clock.now()?,
                );
                store.append_started(started)?;

                let completion = match action {
                    None => Completion {
                        outcome: Outcome::Skipped,
                        output: None,
                        said: None,
                        failure_class: None,
                        ended_at: clock.now()?,
                    },
                    Some(action) => match action.execute(&input, &mut request.shared) {
                        Ok(output) => match step.output_schema.validate(&output) {
                            Ok(()) => Completion {
                                outcome: Outcome::Went,
                                output: Some(output),
                                said: None,
                                failure_class: None,
                                ended_at: clock.now()?,
                            },
                            Err(error) => Completion {
                                outcome: Outcome::Broke,
                                output: None,
                                said: Some(error.to_string()),
                                failure_class: Some("invalid_output".to_owned()),
                                ended_at: clock.now()?,
                            },
                        },
                        Err(error) => Completion {
                            outcome: Outcome::Broke,
                            output: None,
                            said: Some(error.said),
                            failure_class: Some(error.class),
                            ended_at: clock.now()?,
                        },
                    },
                };
                store.close(&request.run_id, &step.id, attempt, epoch, completion)?;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    Store(String),
    Clock(String),
    InvalidRecord(String),
    DuplicateAttempt { step: String, attempt: u32 },
    MissingAttempt { step: String, attempt: u32 },
    AlreadyClosed { step: String, attempt: u32 },
    StaleEpoch { step: String, epoch: u64 },
    UnknownStep(String),
    UnknownAction(String),
    MissingOutput(String),
    Schema(SchemaError),
    Action(ActionError),
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
        }
    }
}

impl Error for FlowError {}

fn decision_from(graph: &Graph, records: &[StepRecord]) -> Result<Decision, FlowError> {
    let mut ready = Vec::new();
    let mut running = Vec::new();
    let mut waiting = Vec::new();
    let mut stopped = Vec::new();
    let mut failed = Vec::new();
    for step in graph.steps() {
        let latest = latest_for(step, records);
        match latest.and_then(|record| record.outcome) {
            Some(Outcome::Went) => continue,
            None if latest.is_some() => running.push(step.id.clone()),
            Some(Outcome::Waiting) => waiting.push(step.id.clone()),
            Some(Outcome::Stopped) => stopped.push(step.id.clone()),
            Some(Outcome::Skipped) => continue,
            Some(Outcome::Broke)
                if latest.is_some_and(|record| record.attempt >= step.max_attempts) =>
            {
                failed.push(step.id.clone());
            }
            Some(Outcome::Broke) | None => {
                if dependencies_went(step, records) {
                    ready.push(step.id.clone());
                }
            }
        }
    }
    if !failed.is_empty() {
        Ok(Decision::Failed(failed))
    } else if !ready.is_empty() {
        Ok(Decision::Ready(ready))
    } else if !running.is_empty() {
        Ok(Decision::Running(running))
    } else if !waiting.is_empty() {
        Ok(Decision::Waiting(waiting))
    } else if !stopped.is_empty() {
        Ok(Decision::Stopped(stopped))
    } else {
        Ok(Decision::Complete)
    }
}

fn dependencies_went(step: &Step, records: &[StepRecord]) -> bool {
    step.deps.iter().all(|dependency| {
        records
            .iter()
            .filter(|record| record.step_id == *dependency)
            .max_by_key(|record| (record.attempt, record.epoch))
            .and_then(|record| record.outcome)
            == Some(Outcome::Went)
    })
}

fn latest_for<'a>(step: &Step, records: &'a [StepRecord]) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step.id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

fn successful_output(step_id: &str, records: &[StepRecord]) -> Result<Value, FlowError> {
    records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went))
        .max_by_key(|record| (record.attempt, record.epoch))
        .and_then(|record| record.output.clone())
        .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Condition, ValueSchema};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Tick(i64);

    impl Clock for Tick {
        fn now(&mut self) -> Result<i64, FlowError> {
            self.0 += 1;
            Ok(self.0)
        }
    }

    struct Echo;

    impl Action for Echo {
        fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
            Ok(input.clone())
        }
    }

    struct FailOnce(Arc<AtomicUsize>);

    impl Action for FailOnce {
        fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(ActionError::new("temporary", "try again"))
            } else {
                Ok(input.clone())
            }
        }
    }

    fn step(id: &str, deps: &[&str], action: &str, max_attempts: u32) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|id| (*id).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::Any,
            when: None,
            action: action.to_owned(),
            max_attempts,
        }
    }

    #[test]
    fn branch_and_join_use_ready_fronts_and_typed_values() {
        let graph = Graph::new(vec![
            step("root", &[], "echo", 1),
            step("left", &["root"], "echo", 1),
            step("right", &["root"], "echo", 1),
            step("join", &["left", "right"], "echo", 1),
        ])
        .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("root".to_owned(), json!({"value": "said is not data"}))]
                .into_iter()
                .collect(),
            gates: vec!["filesystem".to_owned()],
            shared: [("budget".to_owned(), json!(10))].into_iter().collect(),
        };
        let mut store = InMemoryRecordStore::default();
        let result = InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick(0))
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
        let join = store
            .all()
            .iter()
            .find(|record| record.step_id == "join")
            .expect("record della giunzione");
        assert_eq!(join.input["left"]["value"], "said is not data");
        assert_eq!(join.input["right"]["value"], "said is not data");
    }

    #[test]
    fn retry_repeats_only_the_failed_step() {
        let graph = Graph::new(vec![
            step("first", &[], "echo", 1),
            step("retry", &["first"], "flaky", 2),
        ])
        .expect("grafo valido");
        let count = Arc::new(AtomicUsize::new(0));
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        actions.register("flaky", FailOnce(count));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("first".to_owned(), json!("input"))].into_iter().collect(),
            gates: vec![],
            shared: SharedState::new(),
        };
        let mut store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick(0))
            .expect("il secondo tentativo riesce");
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
    fn later_epoch_fences_a_returning_attempt() {
        let mut store = InMemoryRecordStore::default();
        let mut first = StepRecord::started("run", "step", 1, 4, vec![], json!(null), vec![], 1);
        first.outcome = Some(Outcome::Broke);
        first.failure_class = Some("dead".to_owned());
        first.ended_at = Some(2);
        store.records.push(first);
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
                ended_at: 4,
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
        let graph = Graph::new(vec![conditional]).expect("grafo valido");
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
        };
        let mut store = InMemoryRecordStore::default();
        let execution = InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick(0))
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
}
