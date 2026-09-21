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

/// **AN ASK WRITTEN ONLY WHEN FULL IS NEVER TAKEN DOWN.** A reset empties the
/// context and keeps the session, so a request kept from before it repeated a
/// count the session no longer held. Every turn writes what it measured.
#[test]
fn every_turn_writes_the_standing_it_measured() {
    let ask = step("ask-for-a-mandate", "ask");
    assert!(ask["when"].is_null(), "the standing is written whatever it says: {ask}");
    assert_eq!(ask["with"]["value"]["state"]["$from"], "/measure/state");
}

/// **THE EMPTYING IS NOT THE END OF THE RELAY.** The successor is handed its
/// mandate as context and then stands at the prompt, so the relay types one
/// line to start it — and that line hangs from the store saying a successor
/// arrived, never from the emptying alone. Keyed on the emptying it would go
/// into whatever session is standing there when nobody took anything.
#[test]
fn nothing_is_typed_to_a_successor_the_store_has_not_seen_arrive() {
    let arrived = step("empty-a-session-that-handed-on", "arrived");
    assert_eq!(arrived["action"], "mandate_taken");
    assert!(
        arrived["deps"].as_array().expect("deps").iter().any(|dep| dep == "empty"),
        "the arrival is waited for after the emptying, not before it: {arrived}"
    );
    assert_eq!(
        arrived["with"]["not_by"]["$from"], "/handed_on/session",
        "whose taking does not count comes from the mandate, not from this file"
    );

    let free_again = step("empty-a-session-that-handed-on", "free_again");
    assert!(
        free_again["deps"].as_array().expect("deps").iter().any(|dep| dep == "arrived"),
        "the screen is read after the arrival, not before it: {free_again}"
    );

    let wake = step("empty-a-session-that-handed-on", "wake");
    assert_eq!(wake["action"], "type_into_terminal");
    assert!(
        wake["deps"].as_array().expect("deps").iter().any(|dep| dep == "free_again"),
        "the line is typed only into a session the reading found free: {wake}"
    );
    assert!(
        wake["with"]["line"].as_str().is_some_and(|line| !line.is_empty()),
        "a wake with no line typed would be a step that does nothing: {wake}"
    );
}
