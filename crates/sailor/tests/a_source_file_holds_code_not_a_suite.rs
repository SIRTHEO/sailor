//! Where a file's suite is written. `session_cmd.rs` is 3.457 lines of which
//! 2.545 are its test module; Rust puts that module in a file of its own.
//!
//! **THE CEILING IS NOT HERE**: `files_do_not_grow_out_of_scale` holds it, and
//! weighs product apart from judges. ADR-022.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

/// Source files whose test module is written inside them.
const SUITES_INSIDE_A_SOURCE_FILE_TODAY: usize = 163;

const TEST_MODULE: &str = "#[cfg(test)]";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn is_source(name: &str) -> bool {
    name.ends_with(".rs") || name.ends_with(".ts") || name.ends_with(".tsx")
}

fn sources(under: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(under) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "examples" && name != "node_modules" {
                sources(&path, out);
            }
        } else if is_source(&name) {
            out.push(path);
        }
    }
}

/// Where the suite begins, if it begins in this file at all.
fn the_suite_starts_at(text: &str) -> Option<usize> {
    text.lines()
        .position(|line| line.trim_start().starts_with(TEST_MODULE))
}

/// **A DECLARATION IS NOT A BODY.** `mod tests;` points at a file beside this
/// one and is the shape this judge asks for; `mod tests {` is the one it counts.
fn the_suite_is_written_here(text: &str) -> bool {
    let Some(at) = the_suite_starts_at(text) else {
        return false;
    };
    text.lines()
        .skip(at)
        .take(3)
        .any(|line| line.contains("mod ") && line.trim_end().ends_with('{'))
}

fn measure() -> usize {
    let root = root();
    let mut files = Vec::new();
    for place in [
        root.join("crates"),
        root.join("desktop/src-tauri/src"),
        root.join("desktop/src"),
    ] {
        sources(&place, &mut files);
    }
    workspace::measured_against(files.len(), "sources read", 1, "shape asked for");
    files
        .iter()
        .filter(|file| {
            the_suite_is_written_here(&std::fs::read_to_string(file).unwrap_or_default())
        })
        .count()
}

#[test]
fn the_control_a_declaration_is_not_a_body() {
    assert!(the_suite_is_written_here(
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn b() {}\n}"
    ));
    assert!(!the_suite_is_written_here("fn a() {}\n#[cfg(test)]\nmod tests;"));
    assert!(!the_suite_is_written_here("fn a() {}\n"));

}

#[test]
fn no_new_source_file_carries_its_own_suite() {
    let inside = measure();
    match weigh(SUITES_INSIDE_A_SOURCE_FILE_TODAY, inside) {
        Weighed::TreeIsAbove(more) => panic!(
            "source files carrying their suite inside them: {inside} ({more} more than the seed's \
             {SUITES_INSIDE_A_SOURCE_FILE_TODAY}). Write `#[cfg(test)] mod tests;` and put the \
             suite in a file beside this one, the way ledger, toolbox and flow_cmd already do \
             (ADR-022)."
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {SUITES_INSIDE_A_SOURCE_FILE_TODAY} and the tree holds {inside}, \
             {apart} apart: somebody moved suites out without re-measuring; write {inside}"
        ),
        Weighed::Holds => {}
    }
}
