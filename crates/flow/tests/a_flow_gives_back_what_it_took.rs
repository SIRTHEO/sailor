//! A step declared `even_after_a_break` starts once its dependencies have
//! settled, broken ones included, so what a flow took is given back when the
//! work in between failed. The run stays failed whatever it does. ADR-023.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Completion, Decision, ExecutionRequest,
    Executor, FlowFile, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore,
    RunStops, SharedState, Step, StepRecord, SystemClock, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

struct Breaks;

impl Action for Breaks {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Err(ActionError::new("work_failed", "the work exited non-zero"))
    }
}

/// Hands the step to a person, who closes it later from outside the run.
struct HandsOver;

impl Action for HandsOver {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Waiting("a person takes it".to_owned()))
    }
}

fn step(id: &str, action: &str, deps: &[&str]) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        required: false,
        weight: flow::Weight::Light,
        even_after_a_break: false,
        needs: Vec::new(),
    }
}

fn gives_back(id: &str, action: &str, deps: &[&str]) -> Step {
    let mut step = step(id, action, deps);
    step.even_after_a_break = true;
    step
}

fn registry() -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("echo", Echo);
    actions.register("breaks", Breaks);
    actions.register("hands_over", HandsOver);
    actions
}

fn request() -> ExecutionRequest {
    ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: [("lease".to_owned(), json!({"lease": "slot-0"}))]
            .into_iter()
            .collect::<BTreeMap<_, _>>(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    }
}

fn run(graph: &Graph, store: &InMemoryRecordStore) -> Vec<Decision> {
    InProcessExecutor
        .execute(graph, request(), store, &registry(), &SystemClock)
        .expect("the run answers")
        .decisions
}

fn records_of(store: &InMemoryRecordStore, step_id: &str) -> Vec<StepRecord> {
    store
        .all()
        .into_iter()
        .filter(|record| record.step_id == step_id)
        .collect()
}

#[test]
fn the_step_that_gives_back_runs_after_the_work_broke_and_the_run_stays_failed() {
    let graph = Graph::new(vec![
        step("lease", "echo", &[]),
        step("work", "breaks", &["lease"]),
        step("publish", "echo", &["work"]),
        gives_back("release", "echo", &["lease", "publish"]),
    ])
    .expect("a sane graph");
    let store = InMemoryRecordStore::default();

    let decisions = run(&graph, &store);

    let release = records_of(&store, "release");
    assert_eq!(release.len(), 1, "{decisions:?}");
    assert_eq!(release[0].outcome, Some(Outcome::Went));
    assert_eq!(
        release[0].input,
        json!({"lease": {"lease": "slot-0"}, "broken": ["publish"]}),
        "it is handed what closed and told what did not"
    );
    assert!(records_of(&store, "publish").is_empty(), "{decisions:?}");
    assert_eq!(
        decisions.last(),
        Some(&Decision::Failed(vec!["work".to_owned()]))
    );
}

#[test]
fn the_step_that_gives_back_runs_after_a_clean_run_and_hears_nothing_broke() {
    let graph = Graph::new(vec![
        step("lease", "echo", &[]),
        step("work", "echo", &["lease"]),
        gives_back("release", "echo", &["lease", "work"]),
    ])
    .expect("a sane graph");
    let store = InMemoryRecordStore::default();

    let decisions = run(&graph, &store);

    let release = records_of(&store, "release");
    assert_eq!(release.len(), 1, "{decisions:?}");
    assert_eq!(release[0].input["broken"], json!([]));
    assert_eq!(release[0].input["work"], json!({"lease": "slot-0"}));
    assert_eq!(decisions.last(), Some(&Decision::Complete));
}

#[test]
fn a_step_that_gives_back_and_breaks_is_counted_among_the_broken() {
    let graph = Graph::new(vec![
        step("lease", "echo", &[]),
        step("work", "breaks", &["lease"]),
        gives_back("release", "breaks", &["lease", "work"]),
    ])
    .expect("a sane graph");
    let store = InMemoryRecordStore::default();

    let decisions = run(&graph, &store);

    assert_eq!(records_of(&store, "release").len(), 1, "{decisions:?}");
    assert_eq!(
        decisions.last(),
        Some(&Decision::Failed(vec![
            "work".to_owned(),
            "release".to_owned()
        ]))
    );
}

/// A person closes the step the run was parked on, as `sailor step open` and
/// `close` do: a new attempt, opened and closed from outside the executor.
fn a_person_closes(store: &InMemoryRecordStore, step_id: &str, output: Value) {
    let records = store.records("run").expect("the records");
    let last = records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch))
        .expect("the step was handed");
    let (attempt, epoch) = (last.attempt + 1, last.epoch + 1);
    let started = StepRecord::started(
        "run",
        step_id,
        attempt,
        epoch,
        last.deps.clone(),
        last.input.clone(),
        Vec::new(),
        1,
    );
    store.append_started(started).expect("opened");
    let completion = Completion {
        outcome: Outcome::Went,
        output: Some(output),
        said: None,
        failure_class: None,
        refusal: None,
        ran: None,
        ended_at: 2,
        bytes_seen: None,
        bytes_discarded: None,
    };
    store
        .close("run", step_id, attempt, epoch, completion)
        .expect("closed");
}

#[test]
fn a_run_parked_on_a_handed_step_gives_back_once_when_it_is_resumed() {
    let graph = Graph::new(vec![
        step("lease", "echo", &[]),
        step("work", "breaks", &["lease"]),
        step("review", "hands_over", &["lease"]),
        gives_back("release", "echo", &["lease", "work", "review"]),
    ])
    .expect("a sane graph");
    let store = InMemoryRecordStore::default();

    let parked = run(&graph, &store);
    assert_eq!(
        parked.last(),
        Some(&Decision::Waiting(vec!["review".to_owned()])),
        "a run with something still to give back is parked, not failed"
    );
    assert!(records_of(&store, "release").is_empty(), "{parked:?}");

    a_person_closes(&store, "review", json!({"seen": true}));
    let resumed = run(&graph, &store);
    let release = records_of(&store, "release");
    assert_eq!(release.len(), 1, "{resumed:?}");
    assert_eq!(
        release[0].input,
        json!({"lease": {"lease": "slot-0"}, "review": {"seen": true}, "broken": ["work"]})
    );
    assert_eq!(
        resumed.last(),
        Some(&Decision::Failed(vec!["work".to_owned()]))
    );

    let again = run(&graph, &store);
    assert_eq!(again, vec![Decision::Failed(vec!["work".to_owned()])]);
    assert_eq!(records_of(&store, "release").len(), 1, "it gives back once");
}

fn flow_with(release: Value) -> Result<FlowFile, serde_json::Error> {
    serde_json::from_value(json!({
        "id": "gives-back",
        "description": "takes a lease and gives it back",
        "graph": {"steps": [
            {"id": "lease", "deps": [], "input_schema": {"type": "any"},
             "output_schema": {"type": "any"}, "when": null, "action": "echo",
             "max_attempts": 1},
            release
        ]},
        "inputs": {}
    }))
}

fn release(deps: Value) -> Value {
    json!({"id": "release", "deps": deps, "input_schema": {"type": "any"},
           "output_schema": {"type": "any"}, "when": null, "action": "echo",
           "max_attempts": 1, "even_after_a_break": true})
}

#[test]
fn a_flow_that_gives_back_after_nothing_or_hides_a_step_under_the_list_is_refused() {
    flow_with(release(json!(["lease"]))).expect("a step that gives back the lease loads");

    let after_nothing = flow_with(release(json!([]))).expect_err("gives back after nothing");
    assert!(
        after_nothing.to_string().contains("release")
            && after_nothing.to_string().contains("depends on nothing"),
        "{after_nothing}"
    );

    let hidden = serde_json::from_value::<FlowFile>(json!({
        "id": "gives-back",
        "description": "a step called like the list",
        "graph": {"steps": [
            {"id": "broken", "deps": [], "input_schema": {"type": "any"},
             "output_schema": {"type": "any"}, "when": null, "action": "echo",
             "max_attempts": 1},
            release(json!(["broken"]))
        ]},
        "inputs": {}
    }))
    .expect_err("a dependency the list would hide");
    assert!(hidden.to_string().contains("«broken»"), "{hidden}");
}
