//! The catalogue only works for the sentences that ask it. A line written
//! straight into the code is English for everyone, for ever, and no check goes
//! red — which is how «chiuso {tty}» survived a whole pass that claimed to have
//! moved every sentence out. This counts what is still written in, and lets the
//! number fall and never rise.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What `crates/sailor/src` still holds. It goes down as sentences move into
/// `i18n/`, and a rise means a new one was written into the code. Never raise
/// it to make the gate green.
const SENTENCES_STILL_IN_THE_CODE: usize = 243;

/// Words that open a query for the database, not a line for a person.
const A_QUERY_NOT_A_SENTENCE: &[&str] = &[
    "SELECT", "INSERT", "CREATE", "UPDATE", "DELETE", "PRAGMA", "BEGIN", "COMMIT", "WITH",
];

/// Calls whose text is for whoever reads a stack trace, not for whoever typed
/// the command. They are developer's prose and stay in the code.
const SPOKEN_TO_NOBODY: &[&str] = &[".expect(", "panic!", "assert", "unreachable!", "todo!"];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

fn sources_of_the_command_line(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources_of_the_command_line(&path, into);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            into.push(path);
        }
    }
}

/// The line without its trailing comment, aware that a `//` inside a string is
/// not a comment. Prose about a sentence is not a sentence.
fn code_part(line: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut escaped = false;
    let mut letters = line.chars().peekable();
    while let Some(letter) = letters.next() {
        if inside {
            if escaped {
                escaped = false;
            } else if letter == '\\' {
                escaped = true;
            } else if letter == '"' {
                inside = false;
            }
            out.push(letter);
        } else if letter == '"' {
            inside = true;
            out.push(letter);
        } else if letter == '/' && letters.peek() == Some(&'/') {
            break;
        } else {
            out.push(letter);
        }
    }
    out
}

/// The string literals of one line, escapes kept as written.
fn literals(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    let mut escaped = false;
    for letter in code.chars() {
        if inside {
            if escaped {
                escaped = false;
                current.push(letter);
            } else if letter == '\\' {
                escaped = true;
                current.push(letter);
            } else if letter == '"' {
                inside = false;
                found.push(std::mem::take(&mut current));
            } else {
                current.push(letter);
            }
        } else if letter == '"' {
            inside = true;
        }
    }
    found
}

/// A line for a person rather than a name, a path or a format: four words of
/// its own once the `{placeholders}` are taken out, and at least one small
/// letter. It can let something through; it must not accuse wrongly.
fn is_a_sentence(text: &str) -> bool {
    if !text.chars().any(|letter| letter.is_ascii_lowercase()) {
        return false;
    }
    let opening = text.trim_start();
    if A_QUERY_NOT_A_SENTENCE
        .iter()
        .any(|word| opening.to_uppercase().starts_with(word))
    {
        return false;
    }
    let mut bare = String::new();
    let mut depth = 0usize;
    for letter in text.chars() {
        match letter {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(letter),
            _ => {}
        }
    }
    bare.split_whitespace()
        .filter(|word| word.chars().filter(char::is_ascii_alphabetic).count() >= 2)
        .count()
        >= 4
}

/// Where the sentences are, file by file. Everything below a `#[cfg(test)]` is
/// left alone: a test's own prose is written for whoever reads the failure.
fn sentences_of(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            break;
        }
        let code = code_part(line);
        if SPOKEN_TO_NOBODY.iter().any(|call| code.contains(call)) {
            continue;
        }
        for literal in literals(&code) {
            if is_a_sentence(&literal) {
                found.push((number + 1, literal));
            }
        }
    }
    found
}

fn count_them(root: &Path) -> (usize, BTreeMap<String, usize>, Vec<String>) {
    let mut sources = Vec::new();
    sources_of_the_command_line(&root.join("crates/sailor/src"), &mut sources);
    let mut total = 0;
    let mut per_file = BTreeMap::new();
    let mut examples = Vec::new();
    for path in &sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let shown = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let here = sentences_of(&text);
        if here.is_empty() {
            continue;
        }
        total += here.len();
        per_file.insert(shown.clone(), here.len());
        for (number, text) in here.iter().take(2) {
            examples.push(format!(
                "{shown}:{number}  {}",
                text.chars().take(80).collect::<String>()
            ));
        }
    }
    (total, per_file, examples)
}

fn heaviest(per_file: &BTreeMap<String, usize>) -> String {
    let mut rows: Vec<(&String, &usize)> = per_file.iter().collect();
    rows.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
    rows.iter()
        .take(12)
        .map(|(file, count)| format!("{count:6}  {file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_sentences_written_into_the_command_line_only_shrink() {
    let root = repository_root();
    let (total, per_file, _) = count_them(&root);
    assert!(
        !per_file.is_empty(),
        "no source of the command line was read: the count is blind"
    );
    assert!(
        total <= SENTENCES_STILL_IN_THE_CODE,
        "sentences written into the code: {total} (the declared number is \
         {SENTENCES_STILL_IN_THE_CODE}). A sentence written here is English for \
         everyone for ever: give it a `cli.*` key in i18n/en.json and \
         i18n/it.json and ask the catalogue for it. Where they are, heaviest \
         first:\n{}",
        heaviest(&per_file)
    );
    assert!(
        total == SENTENCES_STILL_IN_THE_CODE,
        "sentences written into the code: {total}, and {SENTENCES_STILL_IN_THE_CODE} \
         are declared. {} have moved out: lower the number, or the ground gained \
         is given back the next time somebody writes one in.",
        SENTENCES_STILL_IN_THE_CODE - total
    );
}

/// The absurd control, first. A count that has stopped seeing what it counts is
/// green everywhere, and green would mean the work is done.
#[test]
fn the_count_still_sees_a_sentence_and_still_ignores_what_is_not_one() {
    let spoken = r#"
fn speak() -> String {
    format!("no terminal has checked in yet, and none is expected")
}
"#;
    assert_eq!(
        sentences_of(spoken).len(),
        1,
        "a line written for a person is no longer counted"
    );

    let not_sentences = r#"
const PATH: &str = "crates/sailor/src/lib.rs";
const QUERY: &str = "SELECT tty, worktree FROM terminals WHERE open = 1";
const SHAPE: &str = "{:<10} {:<14} {:<8}";
let _ = value.expect("the lock does not panic when nobody else holds it");
// a comment saying that no terminal has checked in yet is prose, not a sentence
"#;
    assert!(
        sentences_of(not_sentences).is_empty(),
        "something that is not a line for a person was counted: {:?}",
        sentences_of(not_sentences)
    );

    let below_the_tests =
        "#[cfg(test)]\nmod tests {\n    let text = \"this one is only for a failing test\";\n}\n";
    assert!(
        sentences_of(below_the_tests).is_empty(),
        "the prose of a test was counted as a line for a person"
    );
}
