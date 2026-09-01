//! `SAILOR_HOME` means Sailor's configuration home, and nothing else.
//!
//! `ledger::sailor_home` defines it, `one_home_for_everything` makes it a rule
//! for the descriptors, and the price list and the documents send people to it.
//! A second reader that gave it a second meaning would not disagree out loud:
//! it would obey the same word and go somewhere else.

//! **IT WAS NOT TRUE.** `release_cmd::sources_root` read it as *the sources
//! tree*. Anyone declaring their configuration home the way the rest of the
//! product asks would have had the release build from it — and that directory
//! is a git repository too, so cloning HEAD there restores whatever tree it
//! holds: exit 0, the stamp in place, and the morning's work uninstalled.

//! Nothing was set on this machine, so nothing had gone wrong yet. A latent
//! fault is not a smaller one, it is one nobody has met.

use std::path::{Path, PathBuf};

/// The variable, and what it is allowed to mean.
const VARIABLE: &str = "SAILOR_HOME";

/// Who may read it: the one place that defines the home, and the one that
/// applies the same rule to a machine described rather than run.
///
/// The list only shrinks. A new reader is a new meaning until somebody proves
/// otherwise, and proving it means writing it here on purpose.
const WHO_MAY_READ_IT: &[&str] = &["crates/ledger/src/lib.rs", "crates/toolbox/src/lib.rs"];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

/// Only what ships. A test declares environments on purpose, and one that read
/// them would be red on the very thing that proves the rule holds.
fn shipped_sources(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "tests" || name == "target" {
                continue;
            }
            shipped_sources(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

/// The line without the comment that ends it. A comment may name the variable
/// freely — explaining where the home comes from is not reading it.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

#[test]
fn only_the_home_reads_the_home_variable() {
    let root = repository_root();
    let mut sources = Vec::new();
    shipped_sources(&root.join("crates"), &mut sources);
    assert!(
        sources.len() > 20,
        "the sources were not read: {} files found",
        sources.len()
    );

    let mut caught = Vec::new();
    for path in &sources {
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if WHO_MAY_READ_IT.contains(&shown.as_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if code_part(line).contains(VARIABLE) {
                caught.push(format!("{shown}:{}  {}", number + 1, line.trim()));
            }
        }
    }

    assert!(
        caught.is_empty(),
        "{} line(s) read «{VARIABLE}» outside the place that defines what it \
         means. A second reader does not disagree out loud: it obeys the same \
         word and goes somewhere else, and whoever set the variable did what \
         they were told. Ask `ledger::sailor_home()` instead.\n{}",
        caught.len(),
        caught.join("\n")
    );
}

/// Every named reader has to still read it, or the list becomes a place where a
/// meaning keeps its permission after being removed.
#[test]
fn every_named_reader_still_reads_it() {
    let root = repository_root();
    for file in WHO_MAY_READ_IT {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            panic!("«{file}» is named as a reader and is not there");
        };
        assert!(
            text.lines().any(|line| code_part(line).contains(VARIABLE)),
            "«{file}» is named as a reader of «{VARIABLE}» and does not read it \
             any more: take it off the list"
        );
    }
}

/// The check has to be able to catch something, or it is a green light with no
/// bulb behind it.
#[test]
fn the_check_would_catch_a_second_reader() {
    let reading = r#"        env::var_os("SAILOR_HOME"),"#;
    assert!(
        code_part(reading).contains(VARIABLE),
        "the detector no longer sees the line it exists for"
    );
    let explaining = r#"    // the home comes from SAILOR_HOME when it is declared"#;
    assert!(
        !code_part(explaining).contains(VARIABLE),
        "a comment about the variable is not a second meaning, and must stay free"
    );
}
