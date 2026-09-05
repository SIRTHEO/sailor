//! The relay as a sequence: that it loads, that every power it names is
//! registered, and that under the ceiling it does nothing and says so.
//!
//! The file is read at test time and not at compile time. With `include_str!`
//! a deleted flow does not fail a test, it fails the whole crate's build, and
//! whoever hits it sees a broken crate instead of a missing flow.

use flow::{FlowFile, Graph};
use serde_json::Value;
use std::path::PathBuf;

const FLOW_ID: &str = "passa-il-testimone";

fn flow_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|up| up.parent())
        .expect("the crate sits two levels under the root")
        .join("flows")
        .join(format!("{FLOW_ID}.flow.json"))
}

fn flow_text() -> String {
    let path = flow_path();
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn parsed() -> Value {
    serde_json::from_str(&flow_text()).expect("the flow is JSON")
}

fn step(named: &str) -> Value {
    parsed()["graph"]["steps"]
        .as_array()
        .expect("the flow has steps")
        .iter()
        .find(|step| step["id"] == named)
        .unwrap_or_else(|| panic!("«{named}» is not a step of this flow"))
        .clone()
}

#[test]
fn the_flow_loads_and_its_graph_holds_together() {
    let file: FlowFile = serde_json::from_str(&flow_text()).expect("the flow file loads");
    assert_eq!(file.id, FLOW_ID);
    let graph: Graph = serde_json::from_value(parsed()["graph"].clone())
        .expect("a graph with a cycle would not load");
    assert_eq!(graph.steps().len(), 6, "the relay is six steps");
}

#[test]
fn every_power_the_flow_names_is_registered() {
    let registry = registry::registry_in(registry::House::empty(), None, None);
    for step in parsed()["graph"]["steps"].as_array().expect("steps") {
        let named = step["action"].as_str().expect("a step names an action");
        assert!(
            registry.get(named).is_some(),
            "«{named}» is named by the flow and registered nowhere"
        );
    }
}

/// The most expensive fault of the old relay was invisible declining: it handed
/// over 31 times out of 2,834 chances and nobody knew. Here the decision is one
/// `when`, on a step the whole chain hangs from, so a run under the ceiling
/// leaves the measurement deposited and touches nothing.
#[test]
fn under_the_ceiling_the_whole_chain_is_skipped_by_one_condition() {
    let asking = step("chiedi-il-mandato");
    assert_eq!(asking["when"]["kind"], "pointer_equals");
    assert_eq!(asking["when"]["pointer"], "/past_the_ceiling");
    assert_eq!(asking["when"]["value"], Value::Bool(true));
    assert_eq!(
        asking["deps"].as_array().map(Vec::len),
        Some(1),
        "the condition must sit on the one step the rest hangs from"
    );
}

/// The ceiling is a decision about a budget. Inside the node it could not be
/// argued with; in the step it is one number anybody can change.
#[test]
fn the_ceiling_is_written_in_the_step_and_not_hidden_in_a_node() {
    assert!(
        step("misura")["with"]["ceiling"]
            .as_u64()
            .is_some_and(|n| n > 0),
        "the measuring step must declare its own ceiling"
    );
}

/// Nothing here names a product except the one place a product fact belongs:
/// the descriptor id of the command line to be emptied.
#[test]
fn the_only_product_named_is_the_descriptor_to_ask() {
    let text = flow_text().to_lowercase();
    let emptying = step("azzera");
    assert!(
        emptying["with"]["cli"].as_str().is_some(),
        "the emptying step must name which descriptor to ask"
    );
    for step in parsed()["graph"]["steps"].as_array().expect("steps") {
        if step["id"] == "azzera" {
            continue;
        }
        let written = serde_json::to_string(&step["with"]).expect("a with serialises");
        assert!(
            !written.to_lowercase().contains("claude"),
            "«{}» names a product outside the descriptor it should ask: {written}",
            step["id"]
        );
    }
    assert!(
        !text.contains("/clear"),
        "the line that empties a context belongs in the descriptor, never in the flow"
    );
}

/// Whoever measures gets measured: with the mandate taken before it was asked
/// for, a leftover from the previous handover would be handed on as this one's.
#[test]
fn the_mandate_taken_must_be_newer_than_the_moment_it_was_asked_for() {
    let taking = step("raccogli-il-mandato");
    assert_eq!(
        taking["with"]["not_before"]["$from"], "/at",
        "the floor must come from the step that asked, or a stale mandate passes for fresh"
    );
}
