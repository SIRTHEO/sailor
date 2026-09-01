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
    let directory = PathBuf::from("/tmp").join(format!("sr-relay-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create the test directory");
    directory
}

fn registry() -> flow::ActionRegistry {
    let mut found = flow::ActionRegistry::default();
    relay::register_relay(&mut found);
    found
}

fn run(name: &str, input: Value) -> Result<ActionOutcome, flow::ActionError> {
    let registry = registry();
    let node = registry.get(name).expect("the node is registered");
    node.execute(&input, &mut SharedState::new())
}

fn went(outcome: ActionOutcome) -> Value {
    match outcome {
        ActionOutcome::Went(value) => value,
        ActionOutcome::Waiting(reason) => panic!("expected it to run, it waited: {reason}"),
    }
}

fn waiting(outcome: ActionOutcome) -> String {
    match outcome {
        ActionOutcome::Waiting(reason) => reason,
        ActionOutcome::Went(value) => panic!("expected it to wait, it ran: {value}"),
    }
}

#[test]
fn all_four_nodes_are_registered() {
    let registry = registry();
    for name in [
        relay::MEASURE_TERMINAL_ACTION,
        relay::TYPE_INTO_TERMINAL_ACTION,
        relay::EMPTY_TERMINAL_ACTION,
        relay::TAKE_MANDATE_ACTION,
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

/// Waiting and not failing. No mandate yet is the ordinary state between asking
/// for one and getting it, and a step that broke there would turn a handover
/// into a red run every single time.
#[test]
fn no_mandate_left_yet_waits_instead_of_breaking() {
    let directory = scratch("no-mandate");
    let reason = waiting(
        run(
            relay::TAKE_MANDATE_ACTION,
            json!({"tty": "ttys004", "store": directory}),
        )
        .expect("waiting is not an error"),
    );
    assert!(reason.contains("ttys004"), "{reason}");
    let _ = std::fs::remove_dir_all(&directory);
}

/// The same terminal hands over many times. A mandate from the previous
/// handover read as this one's sends the successor back to finished work.
#[test]
fn a_mandate_older_than_this_handover_is_not_taken() {
    let directory = scratch("stale");
    terminal::mandate::write(
        &terminal::mandate::address_in(&directory, "ttys004"),
        &terminal::mandate::Mandate {
            text: "work from the handover before".to_owned(),
            at: 1_000,
        },
    )
    .expect("leave an old mandate");

    let reason = waiting(
        run(
            relay::TAKE_MANDATE_ACTION,
            json!({"tty": "ttys004", "not_before": 2_000, "store": directory}),
        )
        .expect("waiting is not an error"),
    );
    assert!(reason.contains("older"), "{reason}");
    assert!(
        terminal::mandate::read(&terminal::mandate::address_in(&directory, "ttys004")).is_some(),
        "a mandate it refused to take must still be there"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_mandate_taken_is_taken_away() {
    let directory = scratch("taken");
    terminal::mandate::write(
        &terminal::mandate::address_in(&directory, "ttys004"),
        &terminal::mandate::Mandate {
            text: "carry the conduit on".to_owned(),
            at: 3_000,
        },
    )
    .expect("leave a mandate");

    let said = went(
        run(
            relay::TAKE_MANDATE_ACTION,
            json!({"tty": "ttys004", "not_before": 2_000, "store": directory}),
        )
        .expect("taking works"),
    );
    assert_eq!(said["mandate"], json!("carry the conduit on"));
    assert_eq!(
        terminal::mandate::read(&terminal::mandate::address_in(&directory, "ttys004")),
        None,
        "handed on twice is work done twice"
    );
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
    .err()
    .expect("an undeclared command line must refuse");
    assert_eq!(error.class, "reset_not_declared");
    assert!(error.said.contains("codex"), "{}", error.said);
    let _ = std::fs::remove_dir_all(&directory);
}

/// Typing into a terminal nobody is holding is a refusal with a name, not a
/// silent success. A relay that believed it had typed would clear a context
/// that is still full and hand on a mandate nobody received.
#[test]
fn typing_where_nobody_is_accompanying_refuses_by_name() {
    let directory = scratch("nobody");
    let error = run(
        relay::TYPE_INTO_TERMINAL_ACTION,
        json!({"tty": "ttys004", "line": "hello", "store": directory}),
    )
    .err()
    .expect("typing into nothing must refuse");
    assert_eq!(error.class, "nobody_accompanying");
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
