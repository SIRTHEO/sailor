//! **A TEST THAT CANNOT RUN TWICE AT ONCE GOES RED FOR NO REASON OF ITS OWN.**
//! Two releases building side by side deleted each other's fixtures, and the
//! suite blamed the code. A throwaway directory carries the run that dug it.

use std::path::{Path, PathBuf};

/// Every test source under `crates/`, which is where fixtures are dug.
fn test_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root");
    let mut found = Vec::new();
    walk(&root.join("crates"), &mut found);
    found
        .into_iter()
        .filter(|path| path.components().any(|part| part.as_os_str() == "tests"))
        .collect()
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// The text of each function in a source, cut at `fn ` at any indentation. A
/// coarse cut on purpose: what matters is that the name and the discriminator
/// are written near each other, not the exact boundary.
fn functions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("fn ") && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    out.push(current);
    out
}

/// Naming a directory is not digging one: what collides is a fixture made or
/// swept away, not a working directory handed to a process.
fn digs(body: &str) -> bool {
    body.contains("create_dir") || body.contains("remove_dir")
}

/// A name is the run's when something in the same function varies per run.
fn carries_the_run(body: &str) -> bool {
    body.contains("process::id")
        || body.contains("nanos")
        || body.contains("scratch::directory")
        || body.contains("SAILOR_TEST_TMP")
}

#[test]
fn no_test_digs_a_directory_another_run_would_delete() {
    let mut guilty = Vec::new();
    let myself = Path::new(file!()).file_name().unwrap_or_default().to_owned();
    for source in test_sources() {
        if source.file_name() == Some(myself.as_os_str()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&source) else {
            continue;
        };
        for body in functions(&text) {
            if body.contains("temp_dir()") && digs(&body) && !carries_the_run(&body) {
                let named = body
                    .lines()
                    .find(|line| line.trim_start().starts_with("fn "))
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                guilty.push(format!("{}: {named}", source.display()));
            }
        }
    }
    assert!(
        guilty.is_empty(),
        "these dig a throwaway directory two runs at once would share:\n    {}",
        guilty.join("\n    ")
    );
}

/// **THE CONTROL.** With nothing to find, the check above passes for having
/// looked at nothing: this shows it can still see a name that is shared.
#[test]
fn the_check_still_recognises_a_shared_name() {
    let shared = "fn fake_home(name: &str) -> PathBuf {\n    std::env::temp_dir().join(name)\n}\n";
    let owned = "fn fake_home(name: &str) -> PathBuf {\n    std::env::temp_dir().join(format!(\"{name}-{}\", std::process::id()))\n}\n";
    assert!(!carries_the_run(shared));
    assert!(carries_the_run(owned));
    assert_eq!(functions(shared).len(), 1);
}
