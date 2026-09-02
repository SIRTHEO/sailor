//! `.expect()` prints the **`Debug`** of an error, never its `Display`: 1.239
//! of them stood against ten types whose careful sentence no red test had ever
//! shown. The cure is three lines — `Debug` hands the job to `Display` — and
//! this gate keeps it. Errors only: a value type's structural `Debug` is what
//! makes a failed comparison readable, and delegating there would lose detail
//! instead of gaining prose.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

/// Only what ships. A test may derive whatever it likes on a throwaway type.
fn shipped_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "tests" || name == "target" || name == "descriptors" {
                continue;
            }
            shipped_sources(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

/// The name after `for` on an `impl … <trait> for T` line, when the line is the
/// head of an implementation and not prose about one.
fn implemented_for(line: &str, trait_name: &str) -> Option<String> {
    let line = line.trim_start();
    if !line.starts_with("impl") {
        return None;
    }
    let after = line.split_once(&format!("{trait_name} for "))?.1;
    let name: String = after
        .chars()
        .take_while(|letter| letter.is_alphanumeric() || *letter == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The errors of a file: a type that hand-writes `Display` **and** carries the
/// error's job — it says so with `impl std::error::Error`, or its name does.
/// Anything else is a value, and this gate has nothing to say about it.
fn errors_that_write_their_own_sentence(text: &str) -> BTreeSet<String> {
    let displaying: BTreeSet<String> = text
        .lines()
        .filter_map(|line| implemented_for(line, "Display"))
        .collect();
    let declared: BTreeSet<String> = text
        .lines()
        .filter_map(|line| implemented_for(line, "Error"))
        .collect();
    displaying
        .into_iter()
        .filter(|name| name.ends_with("Error") || declared.contains(name))
        .collect()
}

/// The attributes and prose attached to a definition: the contiguous run of
/// lines above it, up to the first that is neither.
fn what_sits_above<'a>(text: &'a str, definition: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with(definition))
    else {
        return Vec::new();
    };
    let mut above = Vec::new();
    for line in lines[..at].iter().rev() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[") || trimmed.starts_with("//") {
            above.push(*line);
        } else {
            break;
        }
    }
    above
}

/// Whether the type's own definition asks the compiler for a `Debug`.
fn derives_debug(text: &str, name: &str) -> bool {
    ["pub enum ", "pub struct ", "enum ", "struct "]
        .iter()
        .flat_map(|shape| what_sits_above(text, &format!("{shape}{name}")))
        .any(|line| line.contains("#[derive(") && line.contains("Debug"))
}

fn hand_writes_debug(text: &str, name: &str) -> bool {
    text.lines()
        .filter_map(|line| implemented_for(line, "Debug"))
        .any(|found| found == name)
}

#[test]
fn no_error_with_a_sentence_of_its_own_derives_debug() {
    let root = repository_root();
    let mut sources = Vec::new();
    shipped_sources(&root.join("crates"), &mut sources);
    assert!(
        sources.len() > 20,
        "the sources were not read: {} files found",
        sources.len()
    );

    let mut caught = Vec::new();
    let mut seen = 0;
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let shown = path.strip_prefix(&root).unwrap_or(path).to_string_lossy();
        for name in errors_that_write_their_own_sentence(&text) {
            seen += 1;
            if derives_debug(&text, &name) {
                caught.push(format!("{shown}  {name}"));
            }
        }
    }

    assert!(
        seen >= 8,
        "only {seen} error(s) with a sentence of their own were found: the scan \
         has stopped seeing them, and green means nothing"
    );
    assert!(
        caught.is_empty(),
        "{} error type(s) write a sentence for a person and then derive the \
         `Debug` that every red test prints instead. Delete `Debug` from the \
         derive and add three lines: `impl fmt::Debug` calling \
         `fmt::Display::fmt(self, out)`.\n{}",
        caught.len(),
        caught.join("\n")
    );
}

/// Removing the derive is only half of it: `.unwrap()` requires `E: Debug`, so
/// a type that loses the derive and gains nothing does not compile. This says
/// the delegation is really there, rather than trusting that it must be.
#[test]
fn every_such_error_hands_its_debug_to_its_display() {
    let root = repository_root();
    let mut sources = Vec::new();
    shipped_sources(&root.join("crates"), &mut sources);

    let mut missing = Vec::new();
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let shown = path.strip_prefix(&root).unwrap_or(path).to_string_lossy();
        for name in errors_that_write_their_own_sentence(&text) {
            if !hand_writes_debug(&text, &name) {
                missing.push(format!("{shown}  {name}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} error type(s) have no `Debug` of their own: either they still derive \
         one, or they have none at all and no `.unwrap()` can name them.\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// The absurd control, first: a file built to be caught has to be caught. Run
/// against the fault it exists for, not against the code that is already
/// right — a scan that has gone blind is green everywhere.
#[test]
fn the_scan_still_catches_the_fault_it_is_there_for() {
    let guilty = r#"
/// What can go wrong.
#[derive(Debug, Clone)]
pub enum SampleError {
    Missing(String),
}

impl fmt::Display for SampleError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "nothing there")
    }
}
"#;
    let found = errors_that_write_their_own_sentence(guilty);
    assert!(
        found.contains("SampleError"),
        "the scan no longer recognises an error that writes its own sentence"
    );
    assert!(
        derives_debug(guilty, "SampleError"),
        "the scan no longer sees the derive it exists to forbid"
    );
    assert!(
        !hand_writes_debug(guilty, "SampleError"),
        "a derived `Debug` was mistaken for a hand-written one"
    );

    let cured = guilty.replace("#[derive(Debug, Clone)]", "#[derive(Clone)]")
        + r#"
impl fmt::Debug for SampleError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, out)
    }
}
"#;
    assert!(
        !derives_debug(&cured, "SampleError"),
        "the cure is not recognised: the gate would stay red on code that is right"
    );
    assert!(
        hand_writes_debug(&cured, "SampleError"),
        "the delegation is not recognised"
    );

    let a_value = r#"
#[derive(Debug, Clone, Copy)]
pub enum Modality {
    Text,
}

impl fmt::Display for Modality {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "text")
    }
}
"#;
    assert!(
        errors_that_write_their_own_sentence(a_value).is_empty(),
        "a value type was taken for an error: its structural `Debug` is what \
         makes a failed comparison readable, and this gate must leave it alone"
    );
}
