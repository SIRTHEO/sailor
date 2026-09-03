//! The shipped local model runner can be asked like any engine, and its
//! counts are read from the pipe it prints them on.

use toolbox::descriptor::{ReadAs, ReadFrom};

#[test]
fn the_shipped_local_runner_declares_how_it_is_asked_and_where_it_counts() {
    let shipped: serde_json::Value =
        serde_json::from_str(toolbox::descriptor::BUILTIN).expect("the shipped descriptors parse");
    let tools = shipped["tools"].as_array().expect("a list");
    let local = tools
        .iter()
        .find(|tool| tool["id"] == "ollama")
        .expect("the local runner is shipped");
    let descriptor: toolbox::descriptor::Descriptor =
        serde_json::from_value(local.clone()).expect("it parses as a descriptor");
    let ask = descriptor.ask.expect("it can be asked");
    assert_eq!(ask.args.first().map(String::as_str), Some("run"));
    let usage = descriptor.usage.expect("it states its counts");
    assert_eq!(usage.read, ReadAs::Text);
    assert_eq!(usage.from, ReadFrom::Stderr, "the counts are on stderr, not beside the answer");
    assert!(usage.input_tokens.is_some() && usage.output_tokens.is_some());
    assert_eq!(descriptor.data_pact, Some(models::pact::DataPact::DoesNotTrain));
}

#[test]
fn a_descriptor_that_says_nothing_reads_from_stdout() {
    let descriptor: toolbox::descriptor::Descriptor = serde_json::from_str(
        r#"{"id": "x", "family": "ai_cli", "usage": {"read": "text", "total_tokens": "(\\d+)"}}"#,
    )
    .expect("parses");
    assert_eq!(descriptor.usage.expect("usage").from, ReadFrom::Stdout);
}
