//! Sailor is agnostic to the engine: a provider's name belongs in a
//! descriptor, never in a string literal the code branches on **and never in
//! the name of a function, type or field**. Both are counted here, and both
//! counts may only fall.

use std::path::{Path, PathBuf};

/// Re-measured exactly when it falls, never raised.
const NAMED_TODAY: usize = 11;

/// Identifiers carrying an engine's name. Same rule, other half of it.
const NAMED_IDENTIFIERS_TODAY: usize = 0;

/// A name is one word of an identifier, never a run of letters inside one:
/// `legacy` does not name `agy`, and `include` does not name a provider.
const ATOMS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "agy",
    "antigravity",
    "ollama",
    "openrouter",
];

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

/// The words of an identifier, split on `_` and on every capital.
fn words_of(identifier: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for letter in identifier.chars() {
        if letter == '_' || letter.is_ascii_uppercase() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        }
        if letter != '_' {
            word.push(letter.to_ascii_lowercase());
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Code with its comments and its string literals taken out: what is left is
/// what the compiler reads, which is where an identifier can hide.
fn only_code(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text.split("#[cfg(test)]").next().unwrap_or("").chars().peekable();
    let mut inside_string = false;
    while let Some(letter) = rest.next() {
        if inside_string {
            if letter == '\\' {
                rest.next();
            } else if letter == '"' {
                inside_string = false;
            }
            continue;
        }
        match letter {
            '"' => inside_string = true,
            '/' if rest.peek() == Some(&'/') => {
                for skipped in rest.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
                out.push(' ');
            }
            other => out.push(other),
        }
    }
    out
}

/// How many identifiers in this file are named after an engine.
fn identifiers_named_in(text: &str) -> usize {
    let code = only_code(text);
    let mut found = 0;
    for identifier in code.split(|letter: char| !letter.is_alphanumeric() && letter != '_') {
        if identifier.is_empty() {
            continue;
        }
        if words_of(identifier).iter().any(|word| ATOMS.contains(&word.as_str())) {
            found += 1;
        }
    }
    found
}

fn measure() -> (usize, Vec<(usize, PathBuf)>) {
    measure_with(named_in)
}

fn measure_with(count_in: fn(&str) -> usize) -> (usize, Vec<(usize, PathBuf)>) {
    let root = root();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    rust_files(&root.join("desktop/src-tauri/src"), &mut files);
    let mut per_file = Vec::new();
    let mut total = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let count = count_in(&text);
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

#[test]
fn the_control_second_a_word_names_an_engine_and_a_run_of_letters_does_not() {
    assert_eq!(identifiers_named_in("fn from_codex_output() {}"), 1);
    assert_eq!(identifiers_named_in("struct ClaudeReader;"), 1);
    assert_eq!(identifiers_named_in("let legacy = 1; let included = 2;"), 0);
    assert_eq!(identifiers_named_in(r#"let name = "codex";"#), 0);
    assert_eq!(identifiers_named_in("// codex reads it this way"), 0);
}

#[test]
fn no_identifier_is_named_after_an_engine() {
    let (total, per_file) = measure_with(identifiers_named_in);
    let where_they_are: Vec<String> = per_file
        .iter()
        .map(|(count, file)| format!("{count:>4}  {}", file.display()))
        .collect();
    assert_eq!(
        total, NAMED_IDENTIFIERS_TODAY,
        "identifiers named after an engine: {total} (the seed is {NAMED_IDENTIFIERS_TODAY}). \
         A reader per provider is the road model independence forbids: the shape belongs in a \
         descriptor.\n{}",
        where_they_are.join("\n")
    );
}
