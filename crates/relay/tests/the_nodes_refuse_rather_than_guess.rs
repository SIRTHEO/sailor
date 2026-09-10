//! What each node does when it does not know, which is the half a relay is
//! judged on.
//!
//! The old relay handed over 31 times out of 2,834 chances and nobody knew,
//! because a declined turn left no trace. Here every refusal is an outcome a
//! step deposits, and every one of them is checked.

use flow::{ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    terminal::scratch::directory(&format!("relay-{name}")).expect("a scratch directory")
}

fn registry() -> flow::ActionRegistry {
    let mut found = flow::ActionRegistry::default();
    relay::register_relay(&mut found);
    found
}

fn run(name: &str, input: Value) -> Result<ActionOutcome, flow::ActionError> {
    let registry = registry();
    let node = registry.get(name).expect("the node is registered");
    node.execute(&input, &SharedState::new())
}

fn went(outcome: ActionOutcome) -> Value {
    match outcome {
        ActionOutcome::Went(value) => value,
        ActionOutcome::Waiting(reason) => panic!("expected it to run, it waited: {reason}"),
        ActionOutcome::NotYet(reason) => panic!("expected it to run, it postponed: {reason}"),
    }
}

/// **AND IT REFUSES `Waiting`, WHICH IS THE POINT.** From here the two look
/// alike — both say "I did nothing" — and behave opposite: one comes back
/// ready, the other parks for good. That was fault 62.
fn not_yet(outcome: ActionOutcome) -> String {
    match outcome {
        ActionOutcome::NotYet(reason) => reason,
        ActionOutcome::Waiting(reason) => {
            panic!("a step nobody handed over must say «not yet», not «waiting»: {reason}")
        }
        ActionOutcome::Went(value) => panic!("expected it to postpone, it ran: {value}"),
    }
}

#[test]
fn all_four_nodes_are_registered() {
    let registry = registry();
    for name in [
        relay::MEASURE_TERMINAL_ACTION,
        relay::TYPE_INTO_TERMINAL_ACTION,
        relay::EMPTY_TERMINAL_ACTION,
        relay::WAIT_FREE_ACTION,
    ] {
        assert!(registry.get(name).is_some(), "«{name}» is not registered");
    }
}

/// A terminal nobody counted is not a terminal that moved no bytes. Answering
/// «full» or «empty» here would both be inventions.
#[test]
fn a_terminal_with_no_count_says_so_instead_of_reading_as_empty() {
    let directory = scratch("uncounted");
    let outcome = run(
        relay::MEASURE_TERMINAL_ACTION,
        json!({"tty": "ttys004", "ceiling": 500000, "store": directory}),
    )
    .expect("measuring an uncounted terminal is not an error");
    let said = went(outcome);
    assert_eq!(said["counted"], json!(false));
    assert_eq!(said["past_the_ceiling"], json!(false));
    assert!(said["why"].is_string(), "it must say why: {said}");
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_terminal_over_its_ceiling_says_so() {
    let directory = scratch("full");
    terminal::tally::write(
        &terminal::tally::address_in(&directory, "ttys004"),
        &terminal::tally::Tally {
            shown: 2_000_000,
            typed: 0,
            at: 1,
        },
    )
    .expect("write a count");

    let under = went(
        run(
            relay::MEASURE_TERMINAL_ACTION,
            json!({"tty": "ttys004", "ceiling": 5_000_000, "store": directory}),
        )
        .expect("measuring works"),
    );
    assert_eq!(under["past_the_ceiling"], json!(false), "{under}");

    let over = went(
        run(
            relay::MEASURE_TERMINAL_ACTION,
            json!({"tty": "ttys004", "ceiling": 100_000, "store": directory}),
        )
        .expect("measuring works"),
    );
    assert_eq!(over["past_the_ceiling"], json!(true), "{over}");
    assert!(over["estimated_tokens"].as_u64().unwrap_or(0) > 0, "{over}");
    let _ = std::fs::remove_dir_all(&directory);
}

/// The whole reason the line lives in a descriptor. A command line nobody has
/// measured must stop the relay by name, not inherit another one's line.
#[test]
fn emptying_a_command_line_nobody_measured_refuses_by_name() {
    let directory = scratch("undeclared");
    let error = run(
        relay::EMPTY_TERMINAL_ACTION,
        json!({"tty": "ttys004", "cli": "codex", "store": directory}),
    )
    .expect_err("an undeclared command line must refuse");
    assert_eq!(error.class, "reset_not_declared");
    assert!(error.said.contains("codex"), "{}", error.said);
    let _ = std::fs::remove_dir_all(&directory);
}

/// Typing into a terminal Sailor does not hold is a refusal with a name, not a
/// silent success. A relay that believed it had typed would clear a context
/// that is still full and hand on a mandate nobody received.
#[test]
fn typing_into_a_terminal_sailor_does_not_hold_refuses_by_name() {
    let directory = scratch("nobody");
    let error = run(
        relay::TYPE_INTO_TERMINAL_ACTION,
        json!({"tty": "ttys004", "line": "hello", "store": directory}),
    )
    .expect_err("typing into nothing must refuse");
    assert_eq!(error.class, "terminal_not_held");
    let _ = std::fs::remove_dir_all(&directory);
}

/// A typo in a hand-written `with` is named before the run spends anything.
#[test]
fn a_field_no_node_knows_is_named_at_check_time() {
    let registry = registry();
    let node = registry
        .get(relay::MEASURE_TERMINAL_ACTION)
        .expect("registered");
    let named = node.unknown_fields(&json!({"tty": "ttys004", "ceiling": 1, "celing": 2}));
    assert_eq!(named, vec!["celing".to_owned()], "{named:?}");
}

/// Whoever measures gets measured: a check that named every field would say the
/// same thing about a `with` that is written correctly.
#[test]
fn a_with_that_is_written_right_is_accused_of_nothing() {
    let registry = registry();
    let node = registry
        .get(relay::MEASURE_TERMINAL_ACTION)
        .expect("registered");
    assert!(node
        .unknown_fields(&json!({"tty": "ttys004", "ceiling": 1, "store": "/tmp"}))
        .is_empty());
}
