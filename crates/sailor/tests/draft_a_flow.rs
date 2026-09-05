//! The shipped `draft-a-flow`: a sketch in, a flow file out, and every name it
//! uses is one the engine answers to.

use flow::system::{load_all, FlowSource};
use flow::FlowFile;

const FLOW_ID: &str = "draft-a-flow";

fn shipped() -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == FLOW_ID)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .expect("the flow is shipped")
}

/// **THE DRAFT IS THE LAST WORD, AND THE VOCABULARY COMES BEFORE THE AUTHOR**:
/// an author writing without the list would name actions from memory.
#[test]
fn the_sketch_is_read_then_written_then_kept_only_if_it_stands() {
    let flow = shipped();
    let steps: Vec<(&str, &str)> = flow
        .graph
        .steps()
        .iter()
        .map(|step| (step.id.as_str(), step.action.as_str()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("trigger", "trigger"),
            ("vocabulary", "action_list"),
            ("author", "external_engine"),
            ("draft", "flow_draft"),
        ]
    );
    let author = &flow.graph.steps()[2];
    assert_eq!(author.deps, vec!["trigger", "vocabulary"]);
    let draft = &flow.graph.steps()[3];
    assert_eq!(
        draft.with.as_ref().and_then(|with| with["flow"]["$json"].as_str()),
        Some("/answer/flow"),
        "one dependency, so the pointer is bare; and as text, so the drafted flow's own references travel as data"
    );
}

/// Every action the flow names is one the default registry has — the same
/// check `flow_draft` applies to what the author writes, applied to the
/// drafter itself.
#[test]
fn every_action_the_drafter_names_is_registered() {
    let registry = registry::default_registry(None, None);
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

/// The author is told the whole vocabulary and the whole shape — a mandate that
/// left either out would get a flow that fails at `flow_draft` every time.
#[test]
fn the_author_is_handed_the_vocabulary_and_the_shape_of_a_flow() {
    let flow = shipped();
    let author = &flow.graph.steps()[2];
    let stdin = serde_json::to_string(&author.with.as_ref().expect("with")["stdin"]).expect("json");
    assert!(stdin.contains("/vocabulary/actions"), "the list of actions is carried in");
    assert!(stdin.contains("/trigger/text"), "the sketch is carried in");
    assert!(stdin.contains("/answer_shape"), "the answer's shape is carried in");
    assert!(stdin.contains("skippable_dependencies"), "the file's shape is spelled out");
    assert!(stdin.contains("THE POINTER RULE"), "one dependency is /field, many are /step/field");
}

/// An orchestration script written for another agent tool is a sketch too, and
/// the author is told how each of its helpers reads as a flow — without the
/// mapping it would keep the words and lose the graph.
#[test]
fn a_script_is_an_accepted_sketch_and_the_author_is_told_how_to_read_it() {
    let flow = shipped();
    let author = &flow.graph.steps()[2];
    let stdin = serde_json::to_string(&author.with.as_ref().expect("with")["stdin"]).expect("json");
    assert!(stdin.contains("IF THE SKETCH IS A SCRIPT"), "a script is named as an accepted sketch");
    assert!(
        stdin.contains("agent(prompt, {schema, label, phase}) is one «external_engine» step"),
        "one call to the model is one engine step"
    );
    assert!(
        stdin.contains("«schema» becomes the step's «answer_shape»"),
        "the schema is the shape of the answer"
    );
    assert!(
        stdin.contains("parallel([...]) is several steps sharing the same «deps»"),
        "parallel branches share their dependencies"
    );
    assert!(
        stdin.contains("pipeline(items, stage1, stage2) is a chain of «deps»"),
        "a pipeline is a chain of dependencies"
    );
    assert!(
        stdin.contains("phase('…') is the step's «phase»"),
        "a phase is the step's own field, not a word hidden in its id"
    );
    assert!(!stdin.contains("has no phase field"), "the mandate does not deny a field the step has");
}

/// The author writes «phase» on the steps of a sketch that names its moments.
#[test]
fn the_author_is_told_a_step_may_carry_its_phase() {
    let flow = shipped();
    let author = &flow.graph.steps()[2];
    let stdin = serde_json::to_string(&author.with.as_ref().expect("with")["stdin"]).expect("json");
    assert!(stdin.contains("«phase» (optional: the short name of the moment"), "the step shape names the field");
}
