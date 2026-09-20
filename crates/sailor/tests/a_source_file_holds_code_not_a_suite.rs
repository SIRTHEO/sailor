//! Two numbers that say the same thing. A file this repository can no longer be
//! read in one sitting is almost always a file whose test module grew inside
//! it: `session_cmd.rs` is 3.457 lines, of which 2.545 are its suite. Rust puts
//! the suite in its own file with `#[cfg(test)] mod tests;`, and three files
//! here already do. ADR-022. Both seeds may only fall.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

/// Source files whose test module is written inside them.
const SUITES_INSIDE_A_SOURCE_FILE_TODAY: usize = 163;

/// Files whose code — the suite not counted — passes the ceiling.
const FILES_PAST_THE_CEILING_TODAY: usize = 11;

/// **NOT A TASTE.** The number the owner named, and the one every long file
/// here passes by more than a little: the shortest of the eleven is 1.032.
const THE_CEILING: usize = 1000;

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

fn code_lines(text: &str) -> usize {
    the_suite_starts_at(text).unwrap_or_else(|| text.lines().count())
}

fn measure() -> (usize, usize, Vec<(usize, PathBuf)>) {
    let root = root();
    let mut files = Vec::new();
    for place in [
        root.join("crates"),
        root.join("desktop/src-tauri/src"),
        root.join("desktop/src"),
    ] {
        sources(&place, &mut files);
    }
    workspace::measured_against(files.len(), "sources read", THE_CEILING, "lines of ceiling");
    let mut inside = 0;
    let mut past = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if the_suite_is_written_here(&text) {
            inside += 1;
        }
        let code = code_lines(&text);
        if code > THE_CEILING {
            past.push((code, file.strip_prefix(&root).unwrap_or(&file).to_path_buf()));
        }
    }
    past.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    (inside, past.len(), past)
}

fn told(past: &[(usize, PathBuf)]) -> String {
    past.iter()
        .map(|(lines, file)| format!("{lines:>5}  {}", file.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_control_a_declaration_is_not_a_body() {
    assert!(the_suite_is_written_here(
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn b() {}\n}"
    ));
    assert!(!the_suite_is_written_here("fn a() {}\n#[cfg(test)]\nmod tests;"));
    assert!(!the_suite_is_written_here("fn a() {}\n"));

    // The suite is not code, whether it is declared or written here.
    assert_eq!(code_lines("one\ntwo\n#[cfg(test)]\nmod tests {\n}\n"), 2);
    assert_eq!(code_lines("one\ntwo\nthree"), 3);
}

#[test]
fn no_new_source_file_carries_its_own_suite() {
    let (inside, _, _) = measure();
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

#[test]
fn no_new_file_passes_the_ceiling() {
    let (_, past, listed) = measure();
    match weigh(FILES_PAST_THE_CEILING_TODAY, past) {
        Weighed::TreeIsAbove(more) => panic!(
            "files whose code passes {THE_CEILING} lines: {past} ({more} more than the seed's \
             {FILES_PAST_THE_CEILING_TODAY}). The suite is not counted here, so this is code \
             that grew: it comes apart into modules that name what they hold \
             (ADR-022).\n{}",
            told(&listed)
        ),
        Weighed::TreeIsBelow(apart) => panic!(
            "the seed says {FILES_PAST_THE_CEILING_TODAY} and the tree holds {past}, {apart} \
             apart: somebody split a file without re-measuring; write {past}\n{}",
            told(&listed)
        ),
        Weighed::Holds => {}
    }
}
