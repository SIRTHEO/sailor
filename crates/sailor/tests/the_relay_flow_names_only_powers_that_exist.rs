//! The relay as a sequence: that its two shipped flows load, that every power
//! they name is registered, and that the destructive one cannot fire alone.
//!
//! The files are read at test time and not at compile time: with `include_str!`
//! a deleted flow fails the whole crate's build, and whoever hits it sees a
//! broken crate instead of a missing flow.

use flow::{FlowFile, Graph};
use serde_json::Value;

/// The one that asks, and the one that empties.
const THE_TWO: &[&str] = &["ask-for-a-mandate", "empty-a-session-that-handed-on"];

fn flow_text(id: &str) -> String {
    flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, text)| (*text).to_owned())
        .unwrap_or_else(|| panic!("«{id}» is not a flow this binary ships"))
}

fn parsed(id: &str) -> Value {
    serde_json::from_str(&flow_text(id)).expect("the flow is JSON")
}

fn step(id: &str, named: &str) -> Value {
    parsed(id)["graph"]["steps"]
        .as_array()
        .expect("the flow has steps")
        .iter()
        .find(|step| step["id"] == named)
        .unwrap_or_else(|| panic!("«{named}» is not a step of «{id}»"))
        .clone()
}

#[test]
fn both_flows_load_and_their_graphs_hold_together() {
    for id in THE_TWO {
        let file: FlowFile = serde_json::from_str(&flow_text(id)).expect("the flow file loads");
        assert_eq!(&file.id, id);
        let graph: Graph = serde_json::from_value(parsed(id)["graph"].clone())
            .expect("a graph with a cycle would not load");
        assert!(!graph.steps().is_empty(), "«{id}» has no steps");
    }
}

#[test]
fn every_power_the_flows_name_is_registered() {
    let registry = registry::registry_in(registry::House::empty(), None, None);
    let mut looked_up = 0;
    for id in THE_TWO {
        let flow = parsed(id);
        for step in flow["graph"]["steps"].as_array().expect("steps") {
            let named = step["action"].as_str().expect("a step names an action");
            assert!(
                registry.get(named).is_some(),
                "«{named}» is named by «{id}» and registered nowhere"
            );
            looked_up += 1;
        }
    }
    workspace::measured(
        looked_up,
        "steps of the relay flows looked up in the registry",
    );
}

/// **NEITHER FLOW NAMES A PRODUCT.** What empties a session and what a free one
/// looks like are facts about one command line, and both are read from the
/// mandate and the descriptor rather than written here.
#[test]
fn neither_flow_names_a_product_nor_the_line_that_empties_one() {
    for id in THE_TWO {
        let text = flow_text(id).to_lowercase();
        assert!(
            !text.contains("claude"),
            "«{id}» names a product: the name belongs in a descriptor"
        );
        assert!(
            !text.contains("/clear"),
            "«{id}» writes the line that empties a context, which belongs in the descriptor"
        );
    }
}

/// **THE DESTRUCTIVE STEP HANGS FROM THE READING.** A terminal that handed
/// nothing on has written nothing down, so emptying it throws the work away.
#[test]
fn nothing_is_emptied_that_did_not_hand_a_mandate_on_first() {
    let emptying = step("empty-a-session-that-handed-on", "empty");
    let deps = emptying["deps"].as_array().expect("the step declares deps");
    assert!(
        deps.iter().any(|dep| dep == "handed_on"),
        "the emptying step must hang from the one that reads the handover: {deps:?}"
    );
    assert_eq!(
        emptying["with"]["cli"]["$from"], "/handed_on/engine",
        "which command line to empty comes from the mandate, not from this file"
    );
}

/// The threshold is a decision about a budget. Inside the node it could not be
/// argued with; in the flow it is one condition anybody can read.
#[test]
fn the_ask_stands_on_one_condition_anybody_can_read() {
    let ask = step("ask-for-a-mandate", "ask");
    assert_eq!(ask["when"]["kind"], "pointer_equals");
    assert_eq!(ask["when"]["pointer"], "/measure/state");
    assert_eq!(ask["when"]["value"], Value::String("oblige".to_owned()));
}
