//! **AN OUTCOME WITH NO STATE IS DRAWN AS «WAITING».** `STATE_OF_OUTCOME` turns
//! an ending into the state a node is drawn in; what is missing stays
//! `undefined` and falls back to waiting, so a step that will never run again
//! is painted as one about to. Worse than a raw name, which is visibly wrong.
//! Both lists are read from source, so neither is copied here.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

/// The variants of an enum, read from the Rust that declares it.
fn variants(source: &str, name: &str) -> Vec<String> {
    let head = format!("pub enum {name} {{");
    let opened = source.find(&head).map(|at| at + head.len());
    let Some(opened) = opened else {
        return Vec::new();
    };
    let closed = source[opened..].find("\n}").map(|at| opened + at);
    let Some(closed) = closed else {
        return Vec::new();
    };
    source[opened..closed]
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('#'))
        .filter_map(|line| line.strip_suffix(','))
        .filter(|word| word.chars().all(|letter| letter.is_ascii_alphanumeric()))
        .map(str::to_owned)
        .collect()
}

/// The keys of the page's map, read from the page. The value's type is written
/// differently here than in `RunConsole`, so the head is matched up to `= {`.
fn keys_of(source: &str, name: &str) -> Vec<String> {
    let head = format!("const {name}: ");
    let at = source.find(&head).map(|at| at + head.len());
    let Some(at) = at else {
        return Vec::new();
    };
    let opened = source[at..].find("{").map(|found| at + found + 1);
    let Some(opened) = opened else {
        return Vec::new();
    };
    let closed = source[opened..].find("\n};").map(|found| opened + found);
    let Some(closed) = closed else {
        return Vec::new();
    };
    source[opened..closed]
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('*') && !line.starts_with("/*"))
        .filter_map(|line| line.split_once(':'))
        .map(|(key, _)| key.trim().trim_matches('"').to_owned())
        .filter(|key| !key.is_empty())
        .collect()
}

#[test]
fn every_outcome_the_engine_writes_has_a_state_the_window_draws() {
    let root = root();
    let engine = std::fs::read_to_string(root.join("crates/flow/src/record.rs"))
        .expect("the engine declares how a step ends");
    let page = std::fs::read_to_string(root.join("desktop/src/runstate.ts"))
        .expect("the page declares the state each ending is drawn in");

    let declared = variants(&engine, "Outcome");
    let mapped = keys_of(&page, "STATE_OF_OUTCOME");

    // Two empty lists are equal, and this test would approve anything — fault 22.
    assert!(
        declared.len() >= 5 && mapped.len() >= 5,
        "one of the two lists could not be read, so nothing was compared: \
         engine {declared:?}, window {mapped:?}"
    );

    let unmapped: Vec<&String> = declared.iter().filter(|one| !mapped.contains(one)).collect();
    assert!(
        unmapped.is_empty(),
        "the engine ends a step with {unmapped:?} and the window has no state for it, \
         so the node falls back to «waiting» — a finished step drawn as a pending one. \
         Engine: {declared:?}; window: {mapped:?}"
    );

    // The other way round: a state for an ending the engine cannot produce is
    // dead paint nobody finds by looking.
    let invented: Vec<&String> = mapped.iter().filter(|one| !declared.contains(one)).collect();
    assert!(
        invented.is_empty(),
        "the window maps {invented:?} to a state, and the engine never ends a step with it. \
         Engine: {declared:?}; window: {mapped:?}"
    );
}
