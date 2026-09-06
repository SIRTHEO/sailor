//! The detection step does not die on a field it never asked for.
//!
//! MEASURED FAULT, and not on a badly written flow: on
//! `what-this-machine-has`, shipped with the product. Inside a directory
//! holding a `sailor.json` it always failed —
//!
//!   unknown field `workdir`, expected one of `descriptor_paths`,
//!   `include_defaults`, `builtin_catalogs`, `family`, `version_probes`
//!
//! — and outside a project it ran. The difference is not in the flow: it is
//! that the executor offers the project root to every step whose declared
//! schema would accept it, and `{"type": "any"}` accepts anything. With
//! `deny_unknown_fields` the action refused its own caller.
//!
//! The two proofs below hold both halves: the field no longer brings the step
//! down, **and** it is not thrown away — a descriptor written relative counts
//! from the root, not from where the process sits.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use toolbox::{DetectToolsAction, Machine};

fn a_folder(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("toolbox-workdir-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("the scratch directory is created");
    root
}

fn descriptor(inside: &std::path::Path, name: &str) -> std::path::PathBuf {
    let file = inside.join(name);
    fs::write(
        &file,
        json!({
            "tools": [{
                "id": "a-name-nobody-installs",
                "family": "tool",
                "label": "it exists only in this test",
                "detect": [{ "command": "a-binary-that-surely-does-not-exist" }]
            }]
        })
        .to_string(),
    )
    .expect("the descriptor is written");
    file
}

fn output(outcome: ActionOutcome) -> serde_json::Value {
    match outcome {
        ActionOutcome::Went(value) => value,
        other => panic!("the step had to go, and instead: {other:?}"),
    }
}

#[test]
fn a_workdir_does_not_make_the_detection_fall() {
    let root = a_folder("falls");
    let file = descriptor(&root, "test.json");

    let outcome = DetectToolsAction::on(Machine::bare(root.clone()))
        .execute(
            &json!({
                "descriptor_paths": [file.display().to_string()],
                "include_defaults": false,
                "version_probes": false,
                "workdir": root.display().to_string(),
            }),
            &SharedState::new(),
        )
        .expect("the executor may add the workdir: it is no wrong input");

    assert_eq!(output(outcome)["total"], 1, "the descriptor was read");
}

/// **AND THE FIELD IS NOT A SINK.** A relative path counts from the declared
/// root. Without this proof "accept it and throw it away" would pass just the
/// same, and a relative descriptor be sought where the process sits — fault 25.
#[test]
fn a_relative_descriptor_counts_from_the_workdir() {
    let root = a_folder("relative");
    fs::create_dir_all(root.join("tools.d")).expect("the subdirectory");
    descriptor(&root.join("tools.d"), "test.json");

    let outcome = DetectToolsAction::on(Machine::bare(root.clone()))
        .execute(
            &json!({
                "descriptor_paths": ["tools.d/test.json"],
                "include_defaults": false,
                "version_probes": false,
                "workdir": root.display().to_string(),
            }),
            &SharedState::new(),
        )
        .expect("the step goes");

    let output = output(outcome);
    assert_eq!(
        output["total"], 1,
        "read from the root, not from the cwd: {}",
        output["problems"]
    );
}
