//! **THE GIVE-BACK READS THE STEP THAT TOOK, NOT THE STEP THAT WORKED.** The
//! integration cut its tree and merged into it in one shell step: a merge that
//! conflicts fails that step, which produces no output, so `release_tree`,
//! reading the path off it, was skipped and the tree stood, locked. Cutting is
//! a step of its own now, so the path is there whatever the merge does: this is
//! the run that used to leak, proved to leak nothing. ADR-026.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Executor, FlowFile, InMemoryRecordStore,
    InProcessExecutor, Outcome, RecordStore, SharedState, SystemClock,
};
use serde_json::{json, Value};

const SHIPPED: &str = include_str!("../system/integrate-on-the-trunk.flow.json");

/// The step whose failure used to take the tree down with it.
const THE_MERGE: &str = "combined";

/// Hands on what it was given: the trigger's own input carries the mandate the
/// request reads.
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

/// Every shell step passes, except the merge, which conflicts the way git does:
/// it exits badly, with nothing to hand on.
struct EveryShellButTheMerge;

impl Action for EveryShellButTheMerge {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let step = shared
            .get(flow::CURRENT_STEP)
            .and_then(Value::as_str)
            .unwrap_or_default();
        if step == THE_MERGE {
            return Err(ActionError::new(
                "work_failed",
                "work/a-change does not merge cleanly onto main",
            ));
        }
        Ok(ActionOutcome::Went(json!({
            "status": "passed",
            "answer": {
                "commit": "c0ffee", "number": 1, "state": "OPEN", "draft": false,
                "trunk": "7a11", "candidate": "ca11", "already": false,
                "at": "2000-01-01T00:00:00Z", "manual_pending": false,
                "pending": [], "pending_digest": "",
            },
        })))
    }
}

#[test]
fn a_merge_that_conflicts_still_gives_the_tree_back() {
    let file: FlowFile = serde_json::from_str(SHIPPED).expect("the shipped flow parses");
    let store = InMemoryRecordStore::default();
    let taken = "/repo-worktrees/run/take_the_tree";
    let mut actions = ActionRegistry::default();
    actions.register("trigger", Echo);
    actions.register(
        "delivery_request",
        Answers(
            json!({"repo": "/repo", "branch": "work/a-change", "remote": "origin",
                       "base": "main", "forge_repo": "owner/repo", "trunk": "main"}),
        ),
    );
    actions.register(
        "store_select",
        Answers(json!({"records": [{"value": {"verdict": "clean", "commit": "c0ffee"}}]})),
    );
    actions.register("store_write", Answers(json!({"written": true})));
    actions.register("shell_check", EveryShellButTheMerge);
    actions.register("handed_to_agent", Answers(json!({"green": true})));
    actions.register(
        "delivery_policy",
        Answers(
            json!({"merge": "allow", "push": "allow", "release": "allow", "forge": "forge",
                       "remote": "origin", "read_from": "main", "asks_publication": "allow",
                       "asks_integration": "allow", "asks_release": "allow"}),
        ),
    );
    actions.register(
        "candidate_gate",
        Answers(json!({"privacy_exit": 0, "ref": "c0ffee"})),
    );
    actions.register("take_a_tree", Answers(json!({ "tree": taken })));
    actions.register("give_a_tree_back", Answers(json!({ "removed": taken })));

    InProcessExecutor
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
            &actions,
            &SystemClock,
        )
        .expect("the run answers");

    let records = store.records("run").expect("the run's records");
    let outcome = |step: &str| {
        records
            .iter()
            .find(|record| record.step_id == step)
            .and_then(|record| record.outcome)
    };

    assert_eq!(
        outcome(THE_MERGE),
        Some(Outcome::Broke),
        "the merge is the step that fails in this run; the run went: {:?}",
        records
            .iter()
            .map(|r| (&r.step_id, &r.outcome))
            .collect::<Vec<_>>(),
    );
    let given_back = outcome("release_tree").unwrap_or_else(|| {
        panic!(
            "release_tree never ran, so the tree this run cut stands: {:?}",
            records.iter().map(|r| &r.step_id).collect::<Vec<_>>()
        )
    });
    assert_eq!(
        given_back,
        Outcome::Went,
        "the tree is given back after a merge that conflicts, and it was not",
    );
}
