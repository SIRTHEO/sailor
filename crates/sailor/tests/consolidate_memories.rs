//! The shipped `consolidate-memories`: every memory read, one engine asked
//! what to keep and what to drop, and the store rewritten in one pass — so
//! the memories do not grow forever. Never run here: it names an engine.

use flow::system::{load_all, FlowSource};
use flow::FlowFile;
use serde_json::{json, Value};

const FLOW_ID: &str = "consolidate-memories";

fn shipped() -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == FLOW_ID)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .expect("the flow is shipped")
}

fn with_of(flow: &FlowFile, step: usize) -> Value {
    flow.graph.steps()[step].with.clone().expect("the step has a with")
}

fn mandate_of(flow: &FlowFile) -> String {
    serde_json::to_string(&with_of(flow, 2)["stdin"]).expect("json")
}

/// **THE LIST COMES BEFORE THE EDITOR, AND THE REPLACEMENT IS THE LAST WORD**:
/// an editor asked without the list would consolidate from memory, and a
/// replacement before the editor would have nothing to write.
#[test]
fn the_memories_are_listed_then_edited_then_replaced() {
    let flow = shipped();
    let steps: Vec<(&str, &str, &[String])> = flow
        .graph
        .steps()
        .iter()
        .map(|step| (step.id.as_str(), step.action.as_str(), step.deps.as_slice()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("trigger", "trigger", &[][..]),
            ("memories", "memory_list", &["trigger".to_owned()][..]),
            ("editor", "external_engine", &["memories".to_owned()][..]),
            ("replace", "memory_replace", &["editor".to_owned()][..]),
        ]
    );
}

/// Every step has one dependency, so every pointer is bare: the editor reads
/// the list as `/memories`, the replacement reads the answer as `/answer/…`.
#[test]
fn the_pointers_follow_the_pointer_rule() {
    let flow = shipped();
    assert!(mandate_of(&flow).contains(r#"{"$json":"/memories"}"#), "the list is carried in whole");
    let replace = with_of(&flow, 3);
    assert_eq!(replace["keep"]["$from"], json!("/answer/keep"));
    assert_eq!(replace["drop"]["$from"], json!("/answer/drop"));
    assert_eq!(replace["provenance"], json!(FLOW_ID), "the store says who consolidated");
}

/// The answer's shape is asked of the engine and enforced on what comes back,
/// and the two copies of it are one shape.
#[test]
fn the_editor_answers_with_what_to_keep_and_what_to_drop() {
    let flow = shipped();
    let editor = with_of(&flow, 2);
    let shape = &editor["answer_shape"];
    assert_eq!(shape["required"], json!(["keep", "drop"]));
    assert_eq!(shape["allow_extra"], json!(false));
    let kept = &shape["properties"]["keep"]["items"];
    assert_eq!(kept["required"], json!(["type", "label", "value"]));
    assert_eq!(
        kept["properties"]["type"]["values"],
        json!(["user", "feedback", "project", "reference"]),
        "a kept memory is one of the four types and nothing else"
    );
    assert_eq!(shape["properties"]["drop"]["items"], json!({"type": "string"}));
    assert_eq!(editor["kind"], json!("writing"));
    assert_eq!(editor["data"], json!("private"), "memories are a person's facts");
    assert!(mandate_of(&flow).contains(r#"{"$json":"/answer_shape"}"#), "the shape is written once");

    let asked: flow::ValueSchema = serde_json::from_value(shape.clone()).expect("the shape asked is a schema");
    let flow::ValueSchema::Object { properties, .. } = &flow.graph.steps()[2].output_schema else {
        panic!("the editor's output is an object")
    };
    assert_eq!(properties.get("answer"), Some(&asked), "the output declares the same shape the engine is asked for");
}

/// The mandate's key sentences, verbatim: merge, drop, keep the type, keep
/// feedback as written. A mandate that lost one would consolidate wrongly and
/// no test would see it.
#[test]
fn the_mandate_says_what_to_merge_what_to_drop_and_what_never_to_reword() {
    let mandate = mandate_of(&shipped());
    for sentence in [
        "You consolidate the memories Sailor keeps",
        "Merge duplicates into one memory",
        "Drop what is stale or contradicted",
        "Keep the type of every memory exactly as it is",
        "keep the wording of feedback verbatim",
        "a label may not appear in both lists",
        "Answer with one JSON object and nothing else",
    ] {
        assert!(mandate.contains(sentence), "the mandate lost «{sentence}»:\n{mandate}");
    }
}

/// Every action the flow names is one the default registry has — checked
/// without a store, the way `flow check` checks it.
#[test]
fn every_action_the_consolidation_names_is_registered() {
    let registry = registry::registry_in(registry::House::empty(), None, None);
    let names = registry.names();
    for step in shipped().graph.steps() {
        assert!(
            names.contains(&step.action.as_str()),
            "«{}» names «{}», which the engine does not have",
            step.id,
            step.action
        );
    }
}

/// Once a day at a quiet hour, light: a consolidation nobody schedules is one
/// nobody runs, and one that runs at a working hour competes with the work.
#[test]
fn the_consolidation_runs_once_a_day_at_a_quiet_hour() {
    let flow = shipped();
    let schedule = serde_json::to_value(flow.schedule.expect("the consolidation has a schedule"))
        .expect("a schedule serialises");
    assert_eq!(
        schedule,
        json!({
            "recurrence": { "kind": "daily_at", "hour": 4, "minute": 30 },
            "weight": "light",
            "perimeter": []
        })
    );
}
