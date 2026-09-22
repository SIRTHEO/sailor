//! A run stopped short of its end, on its wall, by hand, by a check or at its
//! cap, still gives back what it took before it says why it stopped: the work
//! the stop leaves unstarted counts as fallen, as a break would count it.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Condition, Decision,
    ExecutionRequest, Executor, Graph, InMemoryRecordStore, InProcessExecutor, RunStops,
    SharedState, Step, SystemClock, ValueSchema, CURRENT_STEP,
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
            Some("outlast") => {
                std::thread::sleep(std::time::Duration::from_millis(2100));
                Ok(ActionOutcome::Went(json!({})))
            }
            _ => Ok(ActionOutcome::Went(json!({"tree": "/somewhere"}))),
        }
    }
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
        even_after_a_break: false,
    }
}

/// `open` takes something, `work` then `more` use it, `close` gives it back
/// once `more` has settled, and only when `open` did take it.
fn open_work_close(open_says: &str, work_says: &str, stops: RunStops) -> (Vec<Decision>, Vec<String>) {
    let mut close = says("close", &["open", "more"], "go");
    close.even_after_a_break = true;
    close.when = Some(Condition::PointerExists { pointer: "/open".to_owned() });
    let graph = Graph::new(vec![
        says("open", &[], open_says),
        says("work", &["open"], work_says),
        says("more", &["work"], "go"),
        close,
    ])
    .expect("a sane graph");
    let called = Arc::new(Mutex::new(Vec::new()));
    let mut actions = ActionRegistry::default();
    actions.register("act", Act(called.clone()));
    let request = ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops,
    };
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &actions, &SystemClock)
        .expect("the run answers");
    let order = called.lock().expect("the list").clone();
    (execution.decisions, order)
}

fn a_wall_in_a_second() -> RunStops {
    RunStops {
        wall_deadline_at: Some(SystemClock.now().expect("a clock") + 1),
        ..RunStops::default()
    }
}

#[test]
fn a_run_stopped_on_its_wall_still_gives_back_what_it_took() {
    let (decisions, order) = open_work_close("go", "outlast", a_wall_in_a_second());

    assert_eq!(order, ["open", "work", "close"], "{decisions:?}");
    let Some(Decision::Halted { not_started, .. }) = decisions.last() else {
        panic!("it should have stopped on its wall: {decisions:?}");
    };
    assert_eq!(not_started, &["more".to_owned()]);
}

/// A run parked on a person has not ended: whoever resumes it may still need
/// what it took.
#[test]
fn a_run_paused_on_a_person_keeps_what_it_took() {
    let (decisions, order) = open_work_close("go", "wait", RunStops::default());

    assert_eq!(order, ["open", "work"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Waiting(vec!["work".to_owned()])));
}

#[test]
fn nothing_is_given_back_where_nothing_was_taken() {
    let (decisions, order) = open_work_close("break", "go", RunStops::default());

    assert_eq!(order, ["open"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Failed(vec!["open".to_owned()])));
}

#[test]
fn a_run_that_completes_gives_back_last() {
    let (decisions, order) = open_work_close("go", "go", RunStops::default());

    assert_eq!(order, ["open", "work", "more", "close"], "{decisions:?}");
    assert_eq!(decisions.last(), Some(&Decision::Complete));
}
