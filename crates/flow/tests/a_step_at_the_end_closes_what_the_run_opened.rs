//! A step declared `at_the_end` closes what the run opened, whether the run
//! completes or fails, and never while the run is only paused.
//!
//! Before it, the step that removed an integration's tree depended on the
//! merge: every integration that stopped short left its tree and its build on
//! the disk, and only a sweep from outside could find them.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, ExecutionRequest, Executor,
    Graph, InMemoryRecordStore, InProcessExecutor, RunStops, SharedState, Step, SystemClock,
    ValueSchema, CURRENT_STEP,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Does what its input says, and writes down which step asked.
struct Act(Arc<Mutex<Vec<String>>>);

impl Action for Act {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let step = shared.get(CURRENT_STEP).and_then(Value::as_str).unwrap_or("?");
        self.0.lock().expect("the list").push(step.to_owned());
        match input.get("say").and_then(Value::as_str) {
            Some("break") => Err(ActionError::new("check_failed", "it broke")),
            Some("wait") => Ok(ActionOutcome::Waiting("a person takes it".to_owned())),
            _ => Ok(ActionOutcome::Went(json!({"tree": "/somewhere"}))),
        }
    }
}

fn step(id: &str, deps: &[&str]) -> Step {
    says(id, deps, "go")
}

fn says(id: &str, deps: &[&str], word: &str) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: Some(json!({"say": word})),
        when: None,
        action: "act".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        required: false,
        needs: Vec::new(),
        weight: flow::Weight::Light,
        at_the_end: false,
    }
}

/// `open` makes something, `work` then `more` use it, `close` takes it away.
fn open_work_close(open_says: &str, work_says: &str, close_at_the_end: bool) -> (Vec<Decision>, Vec<String>) {
    let mut close = step("close", &["open"]);
    close.at_the_end = close_at_the_end;
    let graph = Graph::new(vec![
        says("open", &[], open_says),
        says("work", &["open"], work_says),
        step("more", &["work"]),
        close,
    ])
    .expect("a sane graph");
    let roots: BTreeMap<String, Value> = BTreeMap::new();
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut actions = ActionRegistry::default();
    actions.register("act", Act(called.clone()));
    let request = ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: roots,
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &actions, &SystemClock)
        .expect("the run answers");
    let order = called.lock().expect("the list").clone();
    (execution.decisions, order)
}

#[test]
fn a_run_that_fails_still_closes_what_it_opened() {
    let (decisions, order) = open_work_close("go", "break", true);

    assert_eq!(order, ["open", "work", "close"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Failed(vec!["work".to_owned()])));
}

/// The same failure without the declaration: the close depends on nothing
/// that broke, yet the run ends on the failure before it is ever reached.
/// This is the leak, measured.
#[test]
fn without_the_declaration_a_close_that_waits_on_the_work_is_never_reached() {
    let mut close = step("close", &["open", "more"]);
    close.at_the_end = false;
    let graph = Graph::new(vec![
        step("open", &[]),
        says("work", &["open"], "break"),
        step("more", &["work"]),
        close,
    ])
    .expect("a sane graph");
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut actions = ActionRegistry::default();
    actions.register("act", Act(called.clone()));
    let roots: BTreeMap<String, Value> = BTreeMap::new();
    let request = ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: roots,
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let store = InMemoryRecordStore::default();
    InProcessExecutor
        .execute(&graph, request, &store, &actions, &SystemClock)
        .expect("the run answers");

    assert_eq!(*called.lock().expect("the list"), ["open", "work"]);
}

#[test]
fn a_run_that_completes_closes_last() {
    let (decisions, order) = open_work_close("go", "go", true);

    assert_eq!(order, ["open", "work", "more", "close"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Complete));
}

/// Without the declaration the close is ready as soon as `open` went, and runs
/// beside the work that still needs what it takes away.
#[test]
fn a_close_not_declared_at_the_end_runs_beside_the_work() {
    let (_, order) = open_work_close("go", "go", false);

    assert_eq!(&order[..1], ["open"]);
    let close = order.iter().position(|step| step == "close").expect("the close ran");
    let more = order.iter().position(|step| step == "more").expect("more ran");
    assert!(close < more, "the close waited for the work anyway: {order:?}");
}

#[test]
fn a_run_paused_on_a_person_keeps_what_it_opened() {
    let (decisions, order) = open_work_close("go", "wait", true);

    assert_eq!(order, ["open", "work"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Waiting(vec!["work".to_owned()])));
}

#[test]
fn nothing_is_closed_where_nothing_was_opened() {
    let (decisions, order) = open_work_close("break", "go", true);

    assert_eq!(order, ["open"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Failed(vec!["open".to_owned()])));
}
