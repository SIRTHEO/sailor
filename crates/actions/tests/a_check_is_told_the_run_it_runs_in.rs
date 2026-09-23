//! A shell check is told the id of the run it runs in, and the flow cannot
//! tell it another: a status that names its run names the one that is running.

use actions::ShellCheckAction;
use flow::{Action, ActionOutcome, SharedState, CURRENT_RUN, RUN_VARIABLE};
use serde_json::{json, Value};

fn the_command_sees(shared: &SharedState, declared: Option<&str>, run: &str) -> bool {
    let mut input = json!({
        "command": format!("test \"${{{RUN_VARIABLE}:-none}}\" = '{run}'"),
        "timeout_secs": 10,
        "accept": ["failed"],
    });
    if let Some(declared) = declared {
        input["env"][RUN_VARIABLE] = json!(declared);
    }
    match ShellCheckAction::new()
        .execute(&input, shared)
        .expect("the check runs")
    {
        ActionOutcome::Went(output) => output["status"] == "passed",
        other => panic!("the check did not answer: {other:?}"),
    }
}

fn in_the_run(run: &str) -> SharedState {
    let mut shared = SharedState::new();
    shared.insert(CURRENT_RUN.to_owned(), Value::String(run.to_owned()));
    shared
}

#[test]
fn the_run_reaches_the_command() {
    assert!(the_command_sees(&in_the_run("a-flow-7"), None, "a-flow-7"));
}

#[test]
fn a_run_the_flow_declares_does_not_cover_the_one_that_runs() {
    let shared = in_the_run("a-flow-7");
    assert!(the_command_sees(&shared, Some("a-flow-1"), "a-flow-7"));
    assert!(!the_command_sees(&shared, Some("a-flow-1"), "a-flow-1"));
}

#[test]
fn outside_a_run_nothing_is_told() {
    assert!(the_command_sees(&SharedState::new(), None, "none"));
}
