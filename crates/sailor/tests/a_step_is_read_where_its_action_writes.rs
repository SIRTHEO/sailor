//! A step's declared output is the shape its action really writes.
//!
//! **A CHECK WRITES A VERDICT, NOT ITS ANSWER.** `shell_check` and
//! `external_engine` hand on
//! `{status, answer, unresolved}`, so a step declaring the answer's own fields
//! at the top asks for something no run will ever produce, and every run of
//! that flow dies at that step. `flow check` cannot see it: it reads what a
//! flow declares, never what an action returns, and answered «would a run
//! start: yes» for two flows that could not take a single step.

use std::path::{Path, PathBuf};
use serde_json::Value;

/// The actions whose output is a verdict wrapping the answer they read.
const WRITE_A_VERDICT: &[&str] = &["shell_check", "external_engine"];

/// What such an action really puts at the top of its output.
const THE_VERDICT_HOLDS: &[&str] = &["status", "answer", "unresolved"];

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

fn shipped_flows(root: &Path) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    for place in ["crates/flow/system", "flows"] {
        let Ok(entries) = std::fs::read_dir(root.join(place)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.to_string_lossy().ends_with(".flow.json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                found.push((
                    format!("{place}/{}", entry.file_name().to_string_lossy()),
                    value,
                ));
            }
        }
    }
    found
}

fn named_properties(schema: Option<&Value>) -> Vec<String> {
    schema
        .and_then(|schema| schema.get("properties"))
        .and_then(Value::as_object)
        .map(|fields| fields.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn a_step_that_writes_a_verdict_is_not_declared_as_its_answer() {
    let root = repository();
    let flows = shipped_flows(&root);
    // THE CONTROL: a judge that read no flow would pass on an empty tree.
    assert!(
        flows.len() > 10,
        "no shipped flow was read: this judge measures nothing"
    );

    let mut measured = 0_usize;
    let mut wrong = Vec::new();
    for (name, flow) in &flows {
        let Some(steps) = flow.pointer("/graph/steps").and_then(Value::as_array) else {
            continue;
        };
        for step in steps {
            let action = step.get("action").and_then(Value::as_str).unwrap_or("");
            if !WRITE_A_VERDICT.contains(&action) {
                continue;
            }
            let declared = named_properties(step.get("output_schema"));
            // A schema naming nothing leaves the shape unclaimed, which is a
            // different decision and not this judge's business.
            if declared.is_empty() {
                continue;
            }
            measured += 1;
            let id = step.get("id").and_then(Value::as_str).unwrap_or("");
            if !declared.iter().any(|field| THE_VERDICT_HOLDS.contains(&field.as_str())) {
                wrong.push(format!(
                    "{name} · step «{id}» ({action}) declares {declared:?}, and none of those is a field of the verdict {THE_VERDICT_HOLDS:?} its action writes"
                ));
            }
        }
    }

    assert!(
        measured > 0,
        "no step with a declared output was found among {} flows: this judge measures nothing",
        flows.len()
    );
    assert!(
        wrong.is_empty(),
        "a step declares an output its action never writes, so every run of it dies there:\n  {}",
        wrong.join("\n  ")
    );
}
