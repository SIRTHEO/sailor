//! What the command line asks the catalogue for, the catalogue has.
//!
//! **THE FAULT THIS CLOSES.** `catalogue::say` falls back to the bare key, on
//! purpose: an invented sentence would read as if a person had written it. But
//! that fallback prints `cli.workspace.written` at whoever typed the command,
//! and nothing goes red. So the keys are checked here instead, before shipping.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The keys the code asks for, found where they are written.
///
/// A literal and not a variable: `say(chosen_key)` would be invisible here, and
/// that is the point at which this check would have to be replaced rather than
/// widened. Nothing in this tree does it yet.
fn keys_the_code_asks_for() -> BTreeSet<String> {
    let mut asked = BTreeSet::new();
    for path in sources() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for call in ["catalogue::say(", "catalogue::try_say("] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(call) {
                rest = &rest[at + call.len()..];
                let after = rest.trim_start();
                let Some(quoted) = after.strip_prefix('"') else {
                    continue;
                };
                if let Some(end) = quoted.find('"') {
                    asked.insert(quoted[..end].to_owned());
                }
            }
        }
    }
    asked
}

fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root");
    let mut found = Vec::new();
    walk(&root.join("crates"), &mut found);
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git") {
                walk(&path, found);
            }
        } else if name.ends_with(".rs") && !name.starts_with("every_sentence_") {
            found.push(path);
        }
    }
}

/// The names a sentence expects to be handed, as `{these}`.
fn holes(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                names.insert(after[..close].to_owned());
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    names
}

#[test]
fn every_key_the_code_asks_for_is_declared_in_every_language() {
    let asked = keys_the_code_asks_for();
    let mut missing = Vec::new();
    for (language, _) in catalogue::LANGUAGES {
        let entries = catalogue::entries(language).expect("a catalogue that parses");
        for key in &asked {
            if !entries.contains_key(key) {
                missing.push(format!("{language} has no {key}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "these would reach whoever typed the command as their own key: {missing:#?}"
    );
}

/// **THE SAME HOLES IN BOTH, OR ONE LANGUAGE SHOWS A GAP.** `fill` leaves a name
/// nobody supplied standing as `{name}`, which is right for a hole and wrong for
/// a translation that renamed one. The caller hands the names of the source
/// language, so any other language declaring different ones prints them raw.
#[test]
fn a_sentence_expects_the_same_names_in_every_language() {
    let source = catalogue::entries(catalogue::SOURCE_LANGUAGE).expect("a source catalogue");
    let mut differing = Vec::new();
    for (language, _) in catalogue::LANGUAGES {
        if *language == catalogue::SOURCE_LANGUAGE {
            continue;
        }
        let entries = catalogue::entries(language).expect("a catalogue that parses");
        for (key, text) in source {
            let Some(other) = entries.get(key) else {
                continue;
            };
            if holes(text) != holes(other) {
                differing.push(format!(
                    "{key}: {} wants {:?}, {language} wants {:?}",
                    catalogue::SOURCE_LANGUAGE,
                    holes(text),
                    holes(other)
                ));
            }
        }
    }
    assert!(differing.is_empty(), "{differing:#?}");
}

/// **THE SECOND LANGUAGE IS REALLY REACHABLE**, which is the whole point and is
/// not proved by the two tests above: they would pass on a catalogue whose
/// Italian was a copy of the English. Asked through `look`, which takes the
/// language as an argument — setting the variable here would decide the language
/// of whatever test happens to run beside this one.
#[test]
fn the_same_key_answers_differently_in_the_two_languages() {
    let key = "cli.workspace.already_declared";
    let values = [("file", "sailor.json")];
    let english = catalogue::look("en", key, &values).expect("the source language has it");
    let italian = catalogue::look("it", key, &values).expect("the other language has it");
    assert!(english.contains("is already there"), "{english}");
    assert!(italian.contains("esiste già"), "{italian}");
    assert_ne!(english, italian, "one of the two is a copy of the other");
}

/// **THE ABSURD CONTROL, FIRST.** A scan that opened nothing would pass both
/// tests above by having found no key to check. It asks for a key this file
/// knows is asked for, so a scan gone blind says so instead of going green.
#[test]
fn the_scan_still_finds_what_it_is_there_to_find() {
    let asked = keys_the_code_asks_for();
    assert!(
        asked.contains("cli.workspace.written"),
        "the scan found {} keys and not the one this test names: it is no longer \
         reading the sources it thinks it is",
        asked.len()
    );
}
