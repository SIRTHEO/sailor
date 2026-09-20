//! The other direction of `every_declared_check_names_a_test_that_exists`.
//!
//! **A GATE NOBODY IS TOLD ABOUT IS FOUND IN THE REPORT.** Nine of fifteen
//! ratchets were not in `docs/gates.md`, one of them a ceiling on file length
//! — which somebody then built twice, having read the list and found none.

use std::path::{Path, PathBuf};

/// What makes a test a ratchet: it carries a seed, or it weighs one.
const A_SEED: &[&str] = &["_TODAY", "weigh("];

const THE_GATES: &str = "docs/gates.md";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn the_ratchets(under: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(under) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_stem()?.to_str()?.to_owned();
            if path.extension()? != "rs" {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            A_SEED.iter().any(|mark| text.contains(mark)).then_some(name)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn every_ratchet_in_the_tree_is_named_in_the_gates() {
    let root = root();
    let ratchets = the_ratchets(&root.join("crates/sailor/tests"));
    let gates = std::fs::read_to_string(root.join(THE_GATES)).expect("the gates");
    workspace::measured_against(ratchets.len(), "ratchets in the tree", 1, "list to name them");

    let unnamed: Vec<&String> = ratchets
        .iter()
        .filter(|name| !gates.contains(&format!("--test {name}")))
        .collect();

    assert!(
        unnamed.is_empty(),
        "{} ratchet(s) can refuse a change that {THE_GATES} never names, so whoever reads the \
         list runs a shorter one than the trunk does and meets the rest in the report:\n{}",
        unnamed.len(),
        unnamed
            .iter()
            .map(|name| format!("    --test {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_control_the_judge_reads_the_list_and_not_its_own_wish() {
    let ratchets = the_ratchets(&root().join("crates/sailor/tests"));
    assert!(
        ratchets.iter().any(|name| name == "comments_do_not_crowd_out_the_code"),
        "a test carrying a seed is a ratchet: {ratchets:?}"
    );
    assert!(
        !ratchets.iter().any(|name| name == "draft_a_flow"),
        "a test carrying no seed is not: {ratchets:?}"
    );
}
