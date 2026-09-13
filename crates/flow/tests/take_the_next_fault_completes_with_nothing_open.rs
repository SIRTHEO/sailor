//! `take-the-next-fault`'s quiet path: nothing is open, `repair` never runs,
//! and `warrant` — declared `required` — never runs either, on the same
//! condition. Before the executor waived a required step ruled out by its own
//! `when`, this run held itself open forever on `warrant`'s side of that same
//! quiet path.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Executor, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, SystemClock,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SHIPPED: &str = include_str!("../system/take-the-next-fault.flow.json");

/// Answers whatever `with.say` names, or the input unchanged. Stands in for
/// `trigger`, `remember` and `external_engine`, none of which this path opens.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

/// The register, read once: nothing open.
struct NothingOpen;

impl Action for NothingOpen {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({"open": false, "register": "empty"})))
    }
}

/// The acceptance and the warrant are both `shell_check`: a real check the
/// quiet path never reaches, since neither `acceptance`'s dependents nor
/// `warrant` itself opens without a fault. Standing in rather than shelling out
/// keeps this a flow-executor test, not a subprocess test.
struct AlwaysPasses;

impl Action for AlwaysPasses {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({"status": "passed"})))
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

fn registry() -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("trigger", Echo);
    actions.register("remember", Echo);
    actions.register("fault_next", NothingOpen);
    actions.register("shell_check", AlwaysPasses);
    actions
}

#[test]
fn nothing_open_completes_the_run_without_opening_repair_or_warrant() {
    let file: FlowFile = serde_json::from_str(SHIPPED).expect("the shipped flow parses");
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &file.graph,
            flow::ExecutionRequest {
                holder: None,
                run_id: "run".to_owned(),
                root_inputs: file.inputs.clone(),
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &registry(),
            &SystemClock,
        )
        .expect("the run answers");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "a night with nothing to fix held the run open: {:?}",
        execution.decisions
    );
    let records = store.records("run").expect("the run's records");
    for step_id in ["repair", "warrant"] {
        let outcome = records
            .iter()
            .find(|record| record.step_id == step_id)
            .and_then(|record| record.outcome);
        assert_eq!(
            outcome,
            Some(Outcome::Skipped),
            "«{step_id}» should have been ruled out by its own condition, not {outcome:?}"
        );
    }
    assert!(
        !records.iter().any(|record| record.step_id == "learn"),
        "«learn» opened on a night with nothing to fix: {records:?}"
    );
}

/// The other side: a fault *is* open, `repair` runs and — since the check
/// fixture above always says «passed» — so does `warrant`, and the run
/// completes for the ordinary reason, not the waiver.
#[test]
fn an_open_fault_still_runs_repair_and_warrant() {
    let mut root_inputs: BTreeMap<String, Value> = BTreeMap::new();
    root_inputs.insert(
        "trigger".to_owned(),
        json!({"text": "cargo test -p flow", "who": "night-shift"}),
    );
    let file: FlowFile = serde_json::from_str(SHIPPED).expect("the shipped flow parses");
    let mut actions = registry();
    actions.register("fault_next", FaultOpen);
    actions.register("external_engine", RepairAnswered);
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &file.graph,
            flow::ExecutionRequest {
                holder: None,
                run_id: "run".to_owned(),
                root_inputs,
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &actions,
            &SystemClock,
        )
        .expect("the run answers");

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "an open fault, reproduced and fixed, did not complete: {:?}",
        execution.decisions
    );
    let records = store.records("run").expect("the run's records");
    for step_id in ["repair", "warrant"] {
        assert!(
            records
                .iter()
                .any(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went)),
            "«{step_id}» did not run on an open fault: {records:?}"
        );
    }
}

struct FaultOpen;

impl Action for FaultOpen {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({
            "open": true,
            "register": "one open",
            "number": 1,
            "happened_on": "2026-01-01",
            "what_happened": "a fixture fault",
            "how_it_showed": "a fixture test",
            "what_would_prevent": "cargo test -p flow",
            "status": "open",
            "remaining": 1,
        })))
    }
}

/// `external_engine`'s real answer shape, so `repair`'s output validates and
/// `warrant` finds `/repair/answer/reproduced` and `/repair/answer/fixed`.
struct RepairAnswered;

impl Action for RepairAnswered {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({
            "status": "ok",
            "answer": {
                "reproduced": true,
                "fixed": true,
                "test": "cargo test -p flow",
                "changed": "nothing, this is a fixture",
                "left_open": "",
            },
        })))
    }
}
