//! What a `bool` never said.
//!
//! The engine judged every `when` and kept one bit: run, or do not. The pointer
//! read, the demand and what was there instead — the whole answer to «why was
//! this not done» — were known then and thrown away.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Condition, ExecutionRequest,
    Executor, FlowError, Graph, InMemoryRecordStore, InProcessExecutor, Outcome,
    RunStops, SharedState, Step, StepRecord, ValueSchema, Why,
};
use serde_json::{json, Value};

/// Hands back what it was given: this test is about the reason, not the work.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

/// A clock that does not move: two readings that agree mean one clock.
struct Stopped;

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(0)
    }
}

fn bare() -> Step {
    Step {
        id: "the-step".to_owned(),
        deps: vec![],
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: "echo".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

fn one_step(when: Condition) -> Graph {
    let mut step = bare();
    step.when = Some(when);
    Graph::new(vec![step]).expect("one step is a graph")
}

fn run_with(graph: &Graph, input: Value) -> Vec<StepRecord> {
    let request = ExecutionRequest {
        run_id: "run".to_owned(),
        root_inputs: [("the-step".to_owned(), input)].into_iter().collect(),
        gates: vec![],
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    actions.register("echo", Echo);
    InProcessExecutor
        .execute(graph, request, &store, &actions, &Stopped)
        .expect("the condition is judged whether or not an action exists");
    store.all()
}

#[test]
fn a_step_that_did_not_run_says_what_it_read_and_what_it_found() {
    let graph = one_step(Condition::PointerHasValue {
        pointer: "/mandate".to_owned(),
    });
    let records = run_with(&graph, json!({"mandate": ""}));
    assert_eq!(records[0].outcome, Some(Outcome::Skipped));

    let Some(Why::Condition(judged)) = &records[0].why else {
        panic!("a step skipped by its condition records no reason: {:?}", records[0].why);
    };
    assert!(!judged.held, "the reason contradicts the outcome");
    assert_eq!(judged.looked_at.as_deref(), Some("/mandate"));
    assert!(
        judged.wanted.contains("carries something"),
        "the reason does not say what was asked: {}",
        judged.wanted
    );
    assert_eq!(
        judged.found,
        Some(json!("")),
        "an empty answer given and nothing at all are two different reasons"
    );
}

/// **A POINTER THAT LEADS NOWHERE IS NOT AN EMPTY VALUE**: one is a step before
/// that answered badly, the other a wire never joined.
#[test]
fn nothing_at_all_reads_differently_from_something_empty() {
    let graph = one_step(Condition::PointerHasValue {
        pointer: "/mandate".to_owned(),
    });
    let records = run_with(&graph, json!({"something_else": "here"}));
    let Some(Why::Condition(judged)) = &records[0].why else {
        panic!("no reason recorded")
    };
    assert_eq!(judged.found, None, "nothing there must not read as empty");
}

/// **JSON SPELLS BOTH OF THEM `null`.** The distinction above lives in memory
/// for as long as the process does; a reader reaches it only through the store,
/// and there it is bytes. Held apart in memory alone is held apart nowhere.
#[test]
fn nothing_there_and_a_null_there_stay_apart_through_the_store() {
    let condition = Condition::PointerHasValue {
        pointer: "/mandate".to_owned(),
    };
    let nothing = Why::Condition(condition.judge(&json!({"something_else": "here"})));
    let a_null = Why::Condition(condition.judge(&json!({"mandate": null})));

    let written = serde_json::to_string(&nothing).expect("a reason serialises");
    let written_null = serde_json::to_string(&a_null).expect("a reason serialises");
    assert_ne!(
        written, written_null,
        "the two reasons were written as the same bytes"
    );

    let Why::Condition(back) = serde_json::from_str::<Why>(&written).expect("a reason reads back");
    assert_eq!(back.found, None, "nothing found came back as a null found");
    let Why::Condition(back) =
        serde_json::from_str::<Why>(&written_null).expect("a reason reads back");
    assert_eq!(
        back.found,
        Some(Value::Null),
        "a null found came back as nothing found"
    );
}

/// Running is a decision too, and costs the same to keep.
#[test]
fn a_step_that_did_run_says_why_it_was_allowed_to() {
    let graph = one_step(Condition::PointerHasValue {
        pointer: "/mandate".to_owned(),
    });
    let records = run_with(&graph, json!({"mandate": "go on"}));
    let Some(Why::Condition(judged)) = &records[0].why else {
        panic!("a step that ran on a condition records no reason")
    };
    assert!(judged.held, "the reason contradicts the outcome");
    assert_eq!(judged.found, Some(json!("go on")));
}

/// **A REASON IS NOT A SECOND COPY OF THE INPUT**: what was read is said in its
/// shape.
#[test]
fn what_was_found_is_kept_in_few_words() {
    let graph = one_step(Condition::PointerHasValue {
        pointer: "/answer".to_owned(),
    });
    let long = "x".repeat(4_000);
    let records = run_with(&graph, json!({"answer": long}));
    let Some(Why::Condition(judged)) = &records[0].why else {
        panic!("no reason recorded")
    };
    let kept = judged.found.as_ref().expect("something was there");
    let written = serde_json::to_string(kept).expect("a value serialises");
    assert!(
        written.len() < 300,
        "the reason carries a second copy of the input: {} bytes",
        written.len()
    );
    assert!(
        written.contains("4000"),
        "the shape must say how much was really there: {written}"
    );
}

/// Nothing to decide, no reason: inventing one would make every plain step
/// claim a judgement nobody made.
#[test]
fn a_step_with_no_condition_claims_no_reason() {
    let graph = Graph::new(vec![bare()]).expect("one step is a graph");
    let records = run_with(&graph, json!({"mandate": "go on"}));
    assert!(records[0].why.is_none());
}
