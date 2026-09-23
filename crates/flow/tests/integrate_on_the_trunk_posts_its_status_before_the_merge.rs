//! `integrate-on-the-trunk` run end to end on stand-in actions that write down
//! the order they ran in. `integrated` posts the status the trunk requires, so
//! it runs only after a person's green and every check before the merge, and
//! the merge runs only after it: a red or pending person posts nothing and
//! merges nothing, and a status the forge refused merges nothing either.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Executor, FlowFile,
    InMemoryRecordStore, InProcessExecutor, SharedState, SystemClock, CURRENT_RUN, CURRENT_STEP,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const SHIPPED: &str = include_str!("../system/integrate-on-the-trunk.flow.json");
const RUN: &str = "integrate-on-the-trunk-1";

#[derive(Clone, Copy, PartialEq)]
enum Person {
    NotAsked,
    Green,
    Red,
    StillAway,
}

type Order = Arc<Mutex<Vec<String>>>;

fn step_of(shared: &SharedState) -> String {
    shared
        .get(CURRENT_STEP)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Answers with a fixed value and writes down the step it answered for.
struct Answers(Value, Order);

impl Action for Answers {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.1.lock().expect("the order").push(step_of(shared));
        if self.0.is_null() {
            return Ok(ActionOutcome::Went(input.clone()));
        }
        Ok(ActionOutcome::Went(self.0.clone()))
    }
}

struct ThePerson(Person, Order);

impl Action for ThePerson {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.1.lock().expect("the order").push(step_of(shared));
        match self.0 {
            Person::NotAsked | Person::Green => Ok(ActionOutcome::Went(json!({"green": true}))),
            Person::Red => Ok(ActionOutcome::Went(json!({"green": false}))),
            Person::StillAway => Ok(ActionOutcome::Waiting("a person has the lines".to_owned())),
        }
    }
}

/// Every `shell_check`: `manual_green` reads what the person said, as the real
/// one does, and `integrated` answers with the run it was told it runs in.
struct Checks {
    person: Person,
    forge_refuses: bool,
    order: Order,
}

impl Action for Checks {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let step = step_of(shared);
        self.order.lock().expect("the order").push(step.clone());
        if step == "manual_green" && !matches!(self.person, Person::Green) {
            return Err(ActionError::new(
                "check_failed",
                "the person did not write green",
            ));
        }
        if step == "integrated" && self.forge_refuses {
            return Err(ActionError::new(
                "check_failed",
                "the forge refused the status",
            ));
        }
        let asked = self.person != Person::NotAsked;
        let run = shared.get(CURRENT_RUN).cloned().unwrap_or(Value::Null);
        Ok(ActionOutcome::Went(json!({
            "status": "passed",
            "answer": {
                "commit": "c0ffee", "number": 1, "state": "OPEN", "draft": false,
                "tree": "/combined", "trunk": "7a11", "candidate": "ca11", "already": false,
                "at": "2000-01-01T00:00:00Z", "manual_pending": asked,
                "pending": if asked { json!(["a line for a person"]) } else { json!([]) },
                "pending_digest": if asked { "d1" } else { "" },
                "on": "c0ffee", "run": run,
            },
        })))
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

fn registry(person: Person, forge_refuses: bool, order: &Order) -> ActionRegistry {
    let answers = |value: Value| Answers(value, order.clone());
    let mut actions = ActionRegistry::default();
    actions.register("trigger", answers(Value::Null));
    actions.register(
        "delivery_request",
        answers(json!({"repo": "/repo", "branch": "work/a-change", "remote": "origin", "base": "main", "forge_repo": "owner/repo", "trunk": "main"})),
    );
    actions.register(
        "store_select",
        answers(json!({"records": [{"value": {"verdict": "clean", "commit": "c0ffee", "checked": [], "lines": []}}]})),
    );
    actions.register("store_write", answers(json!({"written": true})));
    actions.register(
        "shell_check",
        Checks {
            person,
            forge_refuses,
            order: order.clone(),
        },
    );
    actions.register("handed_to_agent", ThePerson(person, order.clone()));
    actions.register(
        "delivery_policy",
        answers(json!({
            "merge": "allow", "push": "allow", "release": "allow", "forge": "forge", "remote": "origin",
            "read_from": "main", "asks_publication": "allow", "asks_integration": "allow", "asks_release": "allow",
        })),
    );
    actions.register(
        "candidate_gate",
        answers(json!({"privacy_exit": 0, "ref": "c0ffee"})),
    );
    actions
}

fn run(person: Person, forge_refuses: bool) -> (Option<Decision>, Vec<String>) {
    let file: FlowFile = serde_json::from_str(SHIPPED).expect("the shipped flow parses");
    let store = InMemoryRecordStore::default();
    let order: Order = Arc::default();
    let execution = InProcessExecutor
        .execute(
            &file.graph,
            flow::ExecutionRequest {
                holder: None,
                run_id: RUN.to_owned(),
                root_inputs: file.inputs.clone(),
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &registry(person, forge_refuses, &order),
            &SystemClock,
        )
        .expect("the run answers");
    let ran = order.lock().expect("the order").clone();
    (execution.decisions.last().cloned(), ran)
}

fn at(ran: &[String], step: &str) -> Option<usize> {
    ran.iter().position(|one| one == step)
}

#[test]
fn the_status_is_posted_after_every_check_and_before_the_merge() {
    let (decision, ran) = run(Person::Green, false);
    assert_eq!(decision, Some(Decision::Complete), "{ran:?}");
    let posted = at(&ran, "integrated").expect("the status was never posted");
    for before in [
        "manual_gates",
        "manual_green",
        "ref_gate",
        "attest",
        "ready",
    ] {
        let there = at(&ran, before).unwrap_or_else(|| panic!("«{before}» never ran: {ran:?}"));
        assert!(
            there < posted,
            "«{before}» ran after the status was posted: {ran:?}"
        );
    }
    let merged = at(&ran, "merge_request").expect("the merge never ran");
    assert!(posted < merged, "the merge ran before the status: {ran:?}");
}

#[test]
fn with_no_line_left_to_a_person_the_status_is_still_posted_before_the_merge() {
    let (decision, ran) = run(Person::NotAsked, false);
    assert_eq!(decision, Some(Decision::Complete), "{ran:?}");
    assert_eq!(at(&ran, "manual_gates"), None, "{ran:?}");
    let posted = at(&ran, "integrated").expect("the status was never posted");
    let merged = at(&ran, "merge_request").expect("the merge never ran");
    assert!(posted < merged, "the merge ran before the status: {ran:?}");
}

#[test]
fn a_red_person_posts_nothing_and_merges_nothing() {
    let (decision, ran) = run(Person::Red, false);
    assert!(
        matches!(decision, Some(Decision::Failed(_))),
        "{decision:?}"
    );
    assert_eq!(at(&ran, "integrated"), None, "{ran:?}");
    assert_eq!(at(&ran, "merge_request"), None, "{ran:?}");
}

#[test]
fn a_person_still_away_posts_nothing_and_merges_nothing() {
    let (decision, ran) = run(Person::StillAway, false);
    assert!(
        matches!(decision, Some(Decision::Waiting(_))),
        "{decision:?}"
    );
    assert_eq!(at(&ran, "integrated"), None, "{ran:?}");
    assert_eq!(at(&ran, "merge_request"), None, "{ran:?}");
}

#[test]
fn a_status_the_forge_refused_merges_nothing() {
    let (decision, ran) = run(Person::Green, true);
    assert!(
        matches!(decision, Some(Decision::Failed(_))),
        "{decision:?}"
    );
    assert!(at(&ran, "integrated").is_some(), "{ran:?}");
    assert_eq!(at(&ran, "merge_request"), None, "{ran:?}");
}
