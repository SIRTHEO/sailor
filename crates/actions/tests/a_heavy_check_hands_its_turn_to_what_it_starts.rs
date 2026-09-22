//! A check running inside the machine's turn hands the turn's token to the
//! command it starts, and only then: a `sailor release` a heavy step starts
//! runs inside that step's turn instead of queueing behind it for ever.

use actions::ShellCheckAction;
use flow::{Action, ActionOutcome, SharedState, MACHINE_TURN, MACHINE_TURN_VARIABLE};
use serde_json::{json, Value};

fn the_command_sees(shared: &SharedState, token: &str) -> bool {
    let input = json!({
        "command": format!("test \"${{{MACHINE_TURN_VARIABLE}:-none}}\" = '{token}'"),
        "timeout_secs": 10,
        "accept": ["failed"],
    });
    match ShellCheckAction::new().execute(&input, shared).expect("the check runs") {
        ActionOutcome::Went(output) => output["status"] == "passed",
        other => panic!("the check did not answer: {other:?}"),
    }
}

#[test]
fn the_token_of_a_heavy_step_reaches_the_command() {
    let mut shared = SharedState::new();
    shared.insert(MACHINE_TURN.to_owned(), Value::String("the-token".to_owned()));

    assert!(the_command_sees(&shared, "the-token"));
}

#[test]
fn a_light_step_hands_on_no_token_of_its_own() {
    assert!(!the_command_sees(&SharedState::new(), "the-token"));
}
