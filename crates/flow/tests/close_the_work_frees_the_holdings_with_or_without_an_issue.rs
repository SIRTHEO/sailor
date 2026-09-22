//! `close-the-work` run end to end on stand-in actions, once for work that
//! named an issue and once for work that named none. The steps that write,
//! judge and send the closing comment open only when an issue was named, and a
//! run with none must still reach the deletion of the branch instead of waiting
//! on a step that never opens.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Executor, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, SystemClock,
};
use serde_json::{json, Value};

const SHIPPED: &str = include_str!("../system/close-the-work.flow.json");

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
/// any of them is read for.
struct Passes;

impl Action for Passes {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({
            "status": "passed",
            "answer": {
                "number": 1,
                "head": "c0ffee",
                "commit": "c0ffee",
                "trunk": "7a11",
                "squashed": false,
                "text_file": "/repo/.git/sailor/outgoing/closing-issue-7.md",
                "sha256": "d1",
                "privacy_exit": 0,
                "issue": "7",
                "state": "CLOSED",
                "tip": "c0ffee",
                "deleted": "c0ffee",
                "archived": "c0ffee",
                "issue_state": "CLOSED",
            },
        })))
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

fn registry(issue: &str) -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("trigger", Echo);
    actions.register(
        "delivery_request",
        Answers(json!({
            "repo": "/repo",
            "branch": "work/a-change",
            "remote": "origin",
            "base": "main",
            "forge_repo": "owner/repo",
            "trunk": "main",
            "issue": issue,
        })),
    );
    actions.register("shell_check", Passes);
    actions.register("close_the_worktree", Answers(json!({"removed": "/tree"})));
    actions.register(
        "archive_the_head",
        Answers(json!({"tag": "refs/tags/archive/work/a-change", "archived": "c0ffee"})),
    );
    actions.register("handed_to_agent", Answers(json!({"authorized": true})));
    actions.register("the_person_said", Answers(json!({"authorized": true})));
    actions.register(
        "delivery_policy",
        Answers(json!({
            "merge": "allow", "push": "allow", "release": "allow", "forge": "forge", "remote": "origin",
            "read_from": "main", "asks_publication": "allow", "asks_integration": "allow", "asks_release": "allow",
        })),
    );
    actions.register(
        "candidate_gate",
        Answers(json!({"privacy_exit": 0, "ref": "c0ffee"})),
    );
    actions
}

fn outcomes(issue: &str) -> (Vec<Decision>, Vec<(String, Option<Outcome>)>) {
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
            &registry(issue),
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
fn work_that_named_no_issue_still_reaches_the_deletion_of_its_branch() {
    let (decisions, records) = outcomes("");
    for step in ["closing_text", "text_gate", "issue"] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Skipped),
            "«{step}» should be ruled out when no issue was named: {records:?}"
        );
    }
    for step in [
        "worktree",
        "local_branch",
        "archive_tag",
        "remote_branch",
        "settled",
    ] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Went),
            "«{step}» never ran for work that named no issue: {records:?}"
        );
    }
    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
}

#[test]
fn work_that_named_an_issue_closes_it_before_the_branch_comes_down() {
    let (decisions, records) = outcomes("7");
    for step in [
        "closing_text",
        "text_gate",
        "issue",
        "worktree",
        "local_branch",
        "archive_tag",
        "remote_branch",
        "settled",
    ] {
        assert_eq!(
            outcome_of(&records, step),
            Some(Outcome::Went),
            "«{step}» did not run for work that named an issue: {records:?}"
        );
    }
    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
}
