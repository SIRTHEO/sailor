//! `integrate-on-the-trunk` run end to end on stand-in actions, once with a
//! line left to a person and once with none. The steps that record a person's
//! green open only when a line was left, and a run with none left must still
//! reach the policy and the merge instead of waiting on a step that never opens.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Executor, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, SystemClock,
};
use serde_json::{json, Value};

const SHIPPED: &str = include_str!("../system/integrate-on-the-trunk.flow.json");

/// Hands its input back: the trigger, whose text the request is read from.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

struct Answers(Value);

impl Action for Answers {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(self.0.clone()))
    }
}

/// Every `shell_check` of the flow answers from one object holding the fields
/// any of them is read for; only `manual_pending` differs between the runs.
struct Passes {
    manual_pending: bool,
}

impl Action for Passes {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({
            "status": "passed",
            "answer": {
                "commit": "c0ffee",
                "number": 1,
                "state": "OPEN",
                "draft": false,
                "tree": "/combined",
                "trunk": "7a11",
                "candidate": "ca11",
                "already": false,
                "at": "2000-01-01T00:00:00Z",
                "manual_pending": self.manual_pending,
                "pending": if self.manual_pending { json!(["a line for a person"]) } else { json!([]) },
                "pending_digest": if self.manual_pending { "d1" } else { "" },
                "on": "c0ffee",
                "run": "run",
            },
        })))
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

fn registry(manual_pending: bool) -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("trigger", Echo);
    actions.register(
        "delivery_request",
        Answers(json!({"repo": "/repo", "branch": "work/a-change", "remote": "origin", "base": "main", "forge_repo": "owner/repo", "trunk": "main"})),
    );
    actions.register("store_select", Answers(json!({"records": [{"value": {"verdict": "clean", "commit": "c0ffee", "checked": [], "lines": []}}]})));
    actions.register("store_write", Answers(json!({"written": true})));
    actions.register("shell_check", Passes { manual_pending });
    actions.register("handed_to_agent", Answers(json!({"green": true})));
    actions.register("delivery_policy", Answers(json!({
            "merge": "allow", "push": "allow", "release": "allow", "forge": "forge", "remote": "origin",
            "read_from": "main", "asks_publication": "allow", "asks_integration": "allow", "asks_release": "allow",
        })));
    actions.register(
        "candidate_gate",
        Answers(json!({"privacy_exit": 0, "ref": "c0ffee"})),
    );
    actions
}

fn outcomes(manual_pending: bool) -> (Vec<Decision>, Vec<(String, Option<Outcome>)>) {
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
            &registry(manual_pending),
            &SystemClock,
        )
        .expect("the run answers");
    let records = store
        .records("run")
        .expect("the run's records")
        .into_iter()
        .map(|record| (record.step_id, record.outcome))
        .collect();
    (execution.decisions, records)
}

fn outcome_of(records: &[(String, Option<Outcome>)], step: &str) -> Option<Outcome> {
    records
        .iter()
        .rev()
        .find(|(id, _)| id == step)
        .and_then(|(_, outcome)| *outcome)
}

#[test]
fn with_no_line_left_to_a_person_the_run_still_reaches_the_merge() {
    let (decisions, records) = outcomes(false);
    for step in ["manual_gates", "manual_green", "manual_recorded"] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Skipped),
            "«{step}» should be ruled out when no line is left: {records:?}"
        );
    }
    for step in ["policy", "merge_request", "remote_carries"] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Went),
            "«{step}» never ran with no line left to a person: {records:?}"
        );
    }
    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
}

#[test]
fn a_person_s_green_is_recorded_before_the_run_goes_on() {
    let (decisions, records) = outcomes(true);
    for step in [
        "manual_gates",
        "manual_green",
        "manual_recorded",
        "policy",
        "merge_request",
    ] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Went),
            "«{step}» did not run with a line left to a person: {records:?}"
        );
    }
    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
}
