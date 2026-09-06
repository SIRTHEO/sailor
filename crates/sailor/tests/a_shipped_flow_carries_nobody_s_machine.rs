//! A flow that ships runs on a machine it has never seen.
//!
//! `flow check` says this already, and says it as a warning nobody has to
//! answer: the flow still runs, only its instructions become false somewhere
//! else. A warning that never falls is how the mandate of the flow the README
//! sends a newcomer to came to be about tidying one person's home directory.

use std::path::{Path, PathBuf};
use serde_json::Value;

/// What a path into somebody's home looks like, whatever their name is.
///
/// `/answer/verdict` is a JSON pointer and not a path, so a bare leading slash
/// says nothing; each shape here names a home or a machine's own scratch.
const SHAPES_OF_A_MACHINE: &[&str] = &["~/", "/Users/", "/home/", "/private/tmp", "/var/folders"];

/// Where the values a step is handed live. A `description` is prose about the
/// flow and may name a directory to explain one; these two are read by the run.
const WHAT_THE_RUN_READS: &[&str] = &["inputs", "with"];

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
                found.push((format!("{place}/{}", entry.file_name().to_string_lossy()), value));
            }
        }
    }
    found
}

/// Every string under a key the run reads, with the pointer that finds it.
fn what_the_run_reads(value: &Value, at: &str, inside: bool, found: &mut Vec<(String, String)>) {
    match value {
        Value::String(text) if inside => found.push((at.to_owned(), text.clone())),
        Value::Object(fields) => {
            for (name, item) in fields {
                let deeper = inside || WHAT_THE_RUN_READS.contains(&name.as_str());
                what_the_run_reads(item, &format!("{at}/{name}"), deeper, found);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                what_the_run_reads(item, &format!("{at}/{index}"), inside, found);
            }
        }
        _ => {}
    }
}

#[test]
fn no_shipped_flow_hands_a_step_a_path_into_somebody_s_home() {
    let root = repository();
    let flows = shipped_flows(&root);
    let mut read = 0usize;
    let mut carried: Vec<String> = Vec::new();
    for (name, flow) in &flows {
        let mut strings = Vec::new();
        what_the_run_reads(flow, "", false, &mut strings);
        read += strings.len();
        for (at, text) in strings {
            for shape in SHAPES_OF_A_MACHINE {
                if let Some(hit) = text.find(shape) {
                    let from = text[..hit].char_indices().rev().nth(30).map_or(0, |(at, _)| at);
                    let to = text[hit..].char_indices().nth(50).map_or(text.len(), |(off, _)| hit + off);
                    carried.push(format!("{name}{at}: …{}…", &text[from..to]));
                }
            }
        }
    }
    workspace::measured_against(
        read,
        "strings a run is handed, in the shipped flows",
        flows.len(),
        "flow files read",
    );
    assert!(
        !flows.is_empty(),
        "no shipped flow was read, and a judge that reads nothing is a judge that says nothing"
    );
    assert!(
        carried.is_empty(),
        "a shipped flow hands a step a path into the machine it was written on, so it \
         means something else — or nothing — anywhere it is downloaded to:\n  {}\n\
         Write what the value is for instead of where it sat, or let the person \
         pass it in: a mandate arrives on the command line, and a directory a step \
         needs is an input, never a sentence.",
        carried.join("\n  ")
    );
}
