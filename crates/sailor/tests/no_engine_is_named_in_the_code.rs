//! Sailor is agnostic to the engine: a provider's name belongs in a
//! descriptor, never in a string literal the code branches on. The names
//! still in the code are counted here, and the count may only fall.

use std::path::{Path, PathBuf};

/// Re-measured exactly when it falls, never raised.
const NAMED_TODAY: usize = 11;

const NAMES: &[&str] = &[
    "claude-code",
    "claude",
    "codex",
    "gemini-cli",
    "gemini",
    "agy",
    "antigravity",
    "ollama",
    "openrouter-cli",
    "openrouter",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn rust_files(under: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(under) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "examples" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" {
            out.push(path);
        }
    }
}

/// The literals `"<name>"` in the code that is not a test.
fn named_in(text: &str) -> usize {
    let code = text.split("#[cfg(test)]").next().unwrap_or("");
    NAMES
        .iter()
        .map(|name| code.matches(&format!("\"{name}\"")).count())
        .sum()
}

fn measure() -> (usize, Vec<(usize, PathBuf)>) {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    rust_files(&root.join("desktop/src-tauri/src"), &mut files);
    let mut per_file = Vec::new();
    let mut total = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let count = named_in(&text);
        if count > 0 {
            total += count;
            per_file.push((count, file.strip_prefix(&root).unwrap_or(&file).to_path_buf()));
        }
    }
    per_file.sort_by(|a, b| b.0.cmp(&a.0));
    (total, per_file)
}

#[test]
fn the_control_first_a_literal_is_counted_and_a_test_module_is_not() {
    assert_eq!(named_in(r#"let x = "codex"; let y = "claude-code";"#), 2);
    assert_eq!(named_in(r#"let x = 1; #[cfg(test)] mod t { const A: &str = "codex"; }"#), 0);
    assert_eq!(named_in(r#"// codex is mentioned in prose only"#), 0);
}

#[test]
fn no_new_engine_name_enters_the_code() {
    let (total, per_file) = measure();
    let where_they_are: Vec<String> = per_file
        .iter()
        .map(|(count, file)| format!("{count:>4}  {}", file.display()))
        .collect();
    assert!(
        total <= NAMED_TODAY,
        "engine names in the code: {total} (the seed is {NAMED_TODAY}). Move the name into a descriptor.\n{}",
        where_they_are.join("\n")
    );
    assert_eq!(
        total, NAMED_TODAY,
        "the seed says {NAMED_TODAY}, the tree holds {total}: somebody pruned without re-measuring; write {total}\n{}",
        where_they_are.join("\n")
    );
}
