//! Whether a session left work running past its turn is read from its record,
//! and how that record says so is a fact about one command line: declared, or
//! unknown, and unknown keeps the session from being emptied.

use toolbox::{Catalog, Descriptor, Source};

fn shipped(id: &str) -> Descriptor {
    Catalog::load(&[Source::Builtin])
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == id)
        .map(|loaded| loaded.descriptor.clone())
        .expect("the descriptor is shipped")
}

#[test]
fn a_declared_line_says_how_its_record_shows_work_left_running() {
    let described: Descriptor = serde_json::from_str(
        r#"{"id":"a","family":"ai_cli","outlives_the_turn":{
            "background_when":["in_back"],"ended_mark":"<id>{id}</id>","schedules":["Later"]}}"#,
    )
    .expect("the descriptor loads");
    let declared = described.outlives_the_turn.expect("declared");
    assert_eq!(declared.background_when, ["in_back"]);
    assert_eq!(declared.ended_mark, "<id>{id}</id>");
    assert_eq!(declared.schedules, ["Later"]);
}

#[test]
fn a_line_that_declares_nothing_is_unknown() {
    let described: Descriptor =
        serde_json::from_str(r#"{"id":"a","family":"ai_cli"}"#).expect("the descriptor loads");
    assert!(described.outlives_the_turn.is_none());
}

/// The line Sailor relays declares both halves of the gate: its record, and the
/// moments its sub-agents start and stop.
#[test]
fn the_shipped_line_that_is_relayed_declares_its_record_and_its_sub_agents() {
    let line = shipped("claude-code");
    assert!(line.outlives_the_turn.is_some());
    assert_eq!(line.event_for("subagent_started"), Some("SubagentStart"));
    assert_eq!(line.event_for("subagent_stopped"), Some("SubagentStop"));
}
