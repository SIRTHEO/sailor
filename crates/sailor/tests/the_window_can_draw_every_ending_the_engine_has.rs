//! **AN ENDING THE WINDOW HAS NO WORD FOR IS AN ENDING SHOWN AS A RAW NAME.**
//! The engine says how a step ended; the page turns that into a word a person
//! reads, from a table of its own. Two hand-written lists confirm each other
//! even when they are wrong together — fault 19 — and here the drift is silent:
//! the page falls back to printing whatever it did not recognise.
//!
//! Both lists are read from source, so neither is copied here. A variant added
//! to the engine turns this red the day it is added, not the day somebody sees
//! `NotYet` on a screen.

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

/// The keys of an object literal the page declares, read from the page.
fn keys_of(source: &str, name: &str) -> Vec<String> {
    let head = format!("export const {name}: Record<string, string> = {{");
    let opened = source.find(&head).map(|at| at + head.len());
    let Some(opened) = opened else {
        return Vec::new();
    };
    let closed = source[opened..].find("\n};").map(|at| opened + at);
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
fn every_ending_the_engine_declares_has_a_word_in_the_window() {
    let root = root();
    let engine = std::fs::read_to_string(root.join("crates/flow/src/record.rs"))
        .expect("the engine declares how a step ends");
    let page = std::fs::read_to_string(root.join("desktop/src/RunConsole.tsx"))
        .expect("the page declares the word for each ending");

    let declared = variants(&engine, "Outcome");
    let drawn = keys_of(&page, "OUTCOME_LABEL");

    // A question that stopped being answerable reads as agreement: two empty
    // lists are equal, and this test would approve anything — fault 22.
    assert!(
        declared.len() >= 5 && drawn.len() >= 5,
        "one of the two lists could not be read, so nothing was compared: \
         engine {declared:?}, window {drawn:?}"
    );

    let missing: Vec<&String> = declared.iter().filter(|one| !drawn.contains(one)).collect();
    assert!(
        missing.is_empty(),
        "the engine can end a step with {missing:?} and the window has no word for it, \
         so it prints the raw name. Engine: {declared:?}; window: {drawn:?}"
    );

    // The other way round, and it is not symmetry for its own sake: a word for
    // an ending the engine cannot produce is a screen written for a world that
    // is gone, and nobody finds out by looking.
    let invented: Vec<&String> = drawn.iter().filter(|one| !declared.contains(one)).collect();
    assert!(
        invented.is_empty(),
        "the window has a word for {invented:?}, which the engine never ends a step with. \
         Engine: {declared:?}; window: {drawn:?}"
    );
}

/// The keys are the variant names because nothing renames them on the way out.
/// The day a `rename_all` arrives, the two lists go on agreeing and the screen
/// stops: this is the canary for that, and it names the fact rather than
/// assuming it.
#[test]
fn an_ending_travels_under_the_name_the_engine_gave_it() {
    let said = serde_json::to_string(&flow::Outcome::Went).expect("an ending serialises");
    assert_eq!(
        said, "\"Went\"",
        "an ending no longer travels under its variant name, so the window's table is \
         keyed on something that never arrives"
    );
}
