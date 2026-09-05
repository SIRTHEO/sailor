//! A run ends on a promise, on a count of turns, by hand, or at a wall.
//!
//! All four meet at one point — the instant a Ready front has been chosen and
//! nothing of it has opened — because that is the only moment where stopping
//! costs nothing. None of them can interrupt a step already running.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Decision, ExecutionRequest,
    Executor, FlowError, Graph, GraphError, InMemoryRecordStore, InProcessExecutor, RunStops,
    SharedState, Step, StopReason, ValueSchema, WALL_REMAINING_SECS,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A stopped clock: what is measured here is a deadline, and a clock moving on
/// its own would make the wall fall for the wrong reason.
struct Stopped(i64);

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0)
    }
}

/// Hands its own input back as its output, so a test can read what a step was
/// given and a promise can be declared over it.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

fn step(id: &str, deps: &[&str]) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
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

fn request(stops: RunStops) -> ExecutionRequest {
    ExecutionRequest {
        run_id: "run".to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops,
    }
}

fn echoes() -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("echo", Echo);
    actions
}

fn opened(store: &InMemoryRecordStore) -> Vec<String> {
    store
        .all()
        .into_iter()
        .map(|record| record.step_id)
        .collect()
}

/// A wall already passed closes the run before it opens anything, and the
/// decision carries the reason rather than a bare "stopped".
#[test]
fn a_run_past_its_wall_closes_stopped_and_the_reason_is_the_wall() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();

    let execution = InProcessExecutor
        .execute(
            &graph,
            request(RunStops {
                wall_deadline_at: Some(0),
                ..RunStops::default()
            }),
            &store,
            &echoes(),
            &Stopped(0),
        )
        .expect("the run ends without an error");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Halted {
            reason: StopReason::Wall,
            not_started: vec!["first".to_owned()],
        })
    );
    assert_eq!(flow::run_status(&execution), ("stopped", false));
    assert_eq!(StopReason::Wall.as_text(), "wall");
    assert!(
        opened(&store).is_empty(),
        "a run at its wall opens nothing at all"
    );
}

/// A wall still ahead does not stop anything, so the check is a deadline and
/// not the mere presence of a wall.
#[test]
fn a_wall_still_ahead_stops_nothing() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();

    let execution = InProcessExecutor
        .execute(
            &graph,
            request(RunStops {
                wall_deadline_at: Some(100),
                ..RunStops::default()
            }),
            &store,
            &echoes(),
            &Stopped(0),
        )
        .expect("the run ends without an error");

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// A step declaring `stops_when` closes the run once its own output makes that
/// pointer true, and the step after it never opens.
#[test]
fn a_promise_a_step_declared_closes_the_run_before_the_next_front() {
    let mut promises = step("first", &[]);
    promises.stops_when = Some("/done".to_owned());
    let graph = Graph::new(vec![promises, step("second", &["first"])]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut inputs = BTreeMap::new();
    inputs.insert("first".to_owned(), json!({"done": true}));

    let mut asking = request(RunStops::default());
    asking.root_inputs = inputs;
    let execution = InProcessExecutor
        .execute(&graph, asking, &store, &echoes(), &Stopped(0))
        .expect("the run ends without an error");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Halted {
            reason: StopReason::Promise,
            not_started: vec!["second".to_owned()],
        })
    );
    assert_eq!(opened(&store), vec!["first".to_owned()]);
}

/// A promise not yet true holds nothing back: the pointer is read, not the
/// declaration.
#[test]
fn a_promise_not_yet_true_holds_nothing_back() {
    let mut promises = step("first", &[]);
    promises.stops_when = Some("/done".to_owned());
    let graph = Graph::new(vec![promises, step("second", &["first"])]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut inputs = BTreeMap::new();
    inputs.insert("first".to_owned(), json!({"done": false}));

    let mut asking = request(RunStops::default());
    asking.root_inputs = inputs;
    let execution = InProcessExecutor
        .execute(&graph, asking, &store, &echoes(), &Stopped(0))
        .expect("the run ends without an error");

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// The turn after the last one declared closes at the start, opening nothing.
#[test]
fn the_turn_after_the_last_one_closes_before_opening_anything() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();

    let execution = InProcessExecutor
        .execute(
            &graph,
            request(RunStops {
                max_turns: Some(3),
                turns_taken: Some(3),
                ..RunStops::default()
            }),
            &store,
            &echoes(),
            &Stopped(0),
        )
        .expect("the run ends without an error");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Halted {
            reason: StopReason::Turns,
            not_started: vec!["first".to_owned()],
        })
    );
    assert!(opened(&store).is_empty());
}

/// The last turn declared still runs: the count stops the n+1th, not the nth.
#[test]
fn the_last_turn_declared_still_runs() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();

    let execution = InProcessExecutor
        .execute(
            &graph,
            request(RunStops {
                max_turns: Some(3),
                turns_taken: Some(2),
                ..RunStops::default()
            }),
            &store,
            &echoes(),
            &Stopped(0),
        )
        .expect("the run ends without an error");

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// A step in a walled flow is told how long it has, so a mandate can say it.
#[test]
fn a_step_in_a_walled_flow_is_told_how_many_seconds_are_left() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut asking = request(RunStops {
        wall_deadline_at: Some(90),
        ..RunStops::default()
    });
    asking
        .root_inputs
        .insert("first".to_owned(), json!({"mandate": "do the thing"}));

    InProcessExecutor
        .execute(&graph, asking, &store, &echoes(), &Stopped(30))
        .expect("the run ends without an error");

    let seen = &store.all()[0].input;
    assert_eq!(
        seen.get("mandate").and_then(Value::as_str),
        Some("do the thing"),
        "what the flow wrote is untouched"
    );
    assert_eq!(
        seen.get(WALL_REMAINING_SECS).and_then(Value::as_i64),
        Some(60),
        "the step is told what is left of the wall, not what the wall was"
    );
}

/// A flow with no wall receives what it always received, key for key.
#[test]
fn a_flow_without_a_wall_is_given_no_such_field() {
    let graph = Graph::new(vec![step("first", &[])]).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut asking = request(RunStops::default());
    asking
        .root_inputs
        .insert("first".to_owned(), json!({"mandate": "do the thing"}));

    InProcessExecutor
        .execute(&graph, asking, &store, &echoes(), &Stopped(30))
        .expect("the run ends without an error");

    assert_eq!(store.all()[0].input.get(WALL_REMAINING_SECS), None);
}

/// A promise that is not a pointer is refused while the graph loads, with the
/// step named: a pointer that can never resolve is a run going to its wall to
/// find out.
#[test]
fn a_promise_that_is_not_a_pointer_is_refused_by_name() {
    let mut promises = step("first", &[]);
    promises.stops_when = Some("done".to_owned());

    let refused = Graph::new(vec![promises]);

    assert_eq!(
        refused,
        Err(GraphError::StopsWhenIsNotAPointer {
            step: "first".to_owned(),
            value: "done".to_owned(),
        })
    );
    let said = refused.unwrap_err().to_string();
    assert!(said.contains("first"), "the refusal names the step: {said}");
    assert!(
        said.contains("stops_when"),
        "and the field it refuses: {said}"
    );
}
