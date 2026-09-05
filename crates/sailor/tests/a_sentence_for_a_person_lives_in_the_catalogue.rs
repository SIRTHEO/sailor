//! The catalogue only works for the sentences that ask it. A line written
//! straight into the code is English for everyone, for ever, and no check goes
//! red — which is how «chiuso {tty}» survived a whole pass that claimed to have
//! moved every sentence out. This counts what is still written in, and lets the
//! number fall and never rise.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the sources under `WHERE_A_PERSON_IS_SPOKEN_TO` still hold. It goes
/// down as sentences move into `i18n/`, and a rise means a new one was written
/// into the code. Never raise it to make the gate green.
const SENTENCES_STILL_IN_THE_CODE: usize = 3;

/// The sources that write lines for a person: the command line, and the
/// counts the window and the command line both print through `ui`.
const WHERE_A_PERSON_IS_SPOKEN_TO: &[&str] = &["crates/sailor/src", "crates/ui/src"];

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

/// Every string literal of a file, with the line it opens on — **the whole
/// file, not a line at a time**: a line-shaped reader stops at the margin and
/// never sees the sentence that runs on past it with a `\`, which is where the
/// long prose lives. Two extractors in a row went blind on that seam. Comments
/// are dropped as it goes and `r"…"` / `r#"…"#` are skipped whole: nothing
/// spoken to a person is written that way.
fn literals_with_their_line(text: &str) -> Vec<(usize, String)> {
    let letters: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0usize;
    let mut line = 1usize;
    while at < letters.len() {
        let letter = letters[at];
        if letter == '\n' {
            line += 1;
            at += 1;
            continue;
        }
        // A comment, to the end of its line.
        if letter == '/' && letters.get(at + 1) == Some(&'/') {
            while at < letters.len() && letters[at] != '\n' {
                at += 1;
            }
            continue;
        }
        // **A CHAR LITERAL IS NOT A STRING, AND `'"'` IS THE ONE THAT BITES.**
        // Read as an opening quote it swallows the code and the doc comments
        // that follow, up to the next `"` anywhere below, and hands the lot
        // back as one sentence nobody wrote. A lifetime falls through: `'a` is
        // not closed by a second quote two characters along.
        if letter == '\'' {
            let closes = match letters.get(at + 1) {
                Some('\\') => at + 3,
                _ => at + 2,
            };
            if letters.get(closes) == Some(&'\'') {
                at = closes + 1;
                continue;
            }
        }
        // A raw string: `r`, some `#`, a quote, up to the same run of `#`.
        if letter == 'r' && matches!(letters.get(at + 1), Some('"') | Some('#')) {
            let mut ahead = at + 1;
            let mut hashes = 0;
            while letters.get(ahead) == Some(&'#') {
                hashes += 1;
                ahead += 1;
            }
            if letters.get(ahead) == Some(&'"') {
                ahead += 1;
                let closing: String = std::iter::once('"')
                    .chain(std::iter::repeat_n('#', hashes))
                    .collect();
                while ahead < letters.len() {
                    if letters[ahead] == '\n' {
                        line += 1;
                    }
                    if letters[ahead..]
                        .iter()
                        .collect::<String>()
                        .starts_with(&closing)
                    {
                        ahead += closing.chars().count();
                        break;
                    }
                    ahead += 1;
                }
                at = ahead;
                continue;
            }
        }
        if letter == '"' {
            let opened_on = line;
            let mut text_of_it = String::new();
            let mut ahead = at + 1;
            let mut escaped = false;
            while ahead < letters.len() {
                let inner = letters[ahead];
                if inner == '\n' {
                    line += 1;
                }
                if escaped {
                    escaped = false;
                } else if inner == '\\' {
                    escaped = true;
                } else if inner == '"' {
                    break;
                }
                text_of_it.push(inner);
                ahead += 1;
            }
            found.push((opened_on, text_of_it));
            at = ahead + 1;
            continue;
        }
        at += 1;
    }
    found
}

/// A literal that runs on past the margin comes back with its `\`, its newline
/// and the indent of the next line inside it. Rust throws all three away, and
/// so does this, or a sentence would be counted as words it does not have.
fn as_the_reader_sees_it(text: &str) -> String {
    let mut out = String::new();
    let mut letters = text.chars().peekable();
    while let Some(letter) = letters.next() {
        if letter != '\\' {
            out.push(letter);
            continue;
        }
        match letters.peek() {
            Some('\n') => {
                letters.next();
                while letters
                    .peek()
                    .is_some_and(|next| *next == ' ' || *next == '\t')
                {
                    letters.next();
                }
            }
            Some('n') | Some('t') => {
                letters.next();
                out.push(' ');
            }
            Some(_) => {
                let escaped = letters.next().expect("peeked");
                out.push(escaped);
            }
            None => out.push(letter),
        }
    }
    out
}

/// What a person reads and a machine does not: `{a placeholder}`, `<a slot>` in
/// a usage line, `[an option]`. What is left is the prose, and only the prose
/// can be said in another language.
fn only_the_prose(text: &str) -> String {
    let mut bare = String::new();
    let mut depth = 0usize;
    for letter in text.chars() {
        match letter {
            '{' | '<' | '[' => depth += 1,
            '}' | '>' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(letter),
            _ => {}
        }
    }
    bare
}

/// A line for a person rather than a name, a path, a format or a usage line:
/// four words of its own prose, and at least one small letter. It can let
/// something through; it must not accuse wrongly — and `sailor worktree create
/// <branch> [name]` is not something anybody can translate.
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
    only_the_prose(text)
        .split_whitespace()
        .filter(|word| word.chars().filter(char::is_ascii_alphabetic).count() >= 2)
        .count()
        >= 4
}

/// Where the sentences are, file by file: the literals of the file, minus the
/// ones a person never reads, minus the ones that are not prose. Everything
/// from `#[cfg(test)]` down is left alone — a test's own prose is written for
/// whoever reads the failure.
fn sentences_of(whole: &str) -> Vec<(usize, String)> {
    let text = match whole.lines().position(|line| line.trim() == "#[cfg(test)]") {
        Some(at) => whole.lines().take(at).collect::<Vec<_>>().join("\n"),
        None => whole.to_owned(),
    };
    let text = text.as_str();
    let lines: Vec<&str> = text.lines().collect();
    // The call is on the line the literal opens on, or on the one above when
    // the argument was pushed down to fit.
    let spoken_to_nobody = |number: usize| {
        let mut around = Vec::new();
        if number >= 2 {
            around.push(lines[number - 2]);
        }
        if let Some(line) = lines.get(number - 1) {
            around.push(line);
        }
        around
            .iter()
            .any(|line| SPOKEN_TO_NOBODY.iter().any(|call| line.contains(call)))
    };
    // **WHAT IS TYPED IS NOT PROSE, AND THE FIELD SAYS WHICH IS WHICH.** The
    // `form` of a `Form` is `sailor faults status <n> <text>`: four lowercase
    // words, which the counter would call a sentence. It is not a hole in the
    // rule but a reading of the code — a `form` that stopped being a shape
    // fails `every_command_says_how_it_is_written_and_names_itself` in
    // `lib.rs`, which wants every one of them to start with `sailor <name>`.
    let is_a_shape_to_type = |number: usize| {
        lines
            .get(number - 1)
            .is_some_and(|line| line.trim_start().starts_with("form: \""))
    };
    literals_with_their_line(text)
        .into_iter()
        .filter(|(number, _)| !spoken_to_nobody(*number))
        .filter(|(number, _)| !is_a_shape_to_type(*number))
        .map(|(number, raw)| (number, as_the_reader_sees_it(&raw)))
        .filter(|(_, text)| is_a_sentence(text))
        .collect()
}

fn count_them(root: &Path) -> (usize, BTreeMap<String, usize>, Vec<String>) {
    let mut sources = Vec::new();
    for place in WHERE_A_PERSON_IS_SPOKEN_TO {
        sources_of_the_command_line(&root.join(place), &mut sources);
    }
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
    for place in WHERE_A_PERSON_IS_SPOKEN_TO {
        let mut sources = Vec::new();
        sources_of_the_command_line(&root.join(place), &mut sources);
        assert!(
            !sources.is_empty(),
            "no source under {place} was read: the count is blind there"
        );
    }
    let (total, per_file, _) = count_them(&root);
    assert!(
        !per_file.is_empty(),
        "no source of the command line was read: the count is blind"
    );
    assert!(
        total <= SENTENCES_STILL_IN_THE_CODE,
        "sentences written into the code: {total} (the declared number is \
         {SENTENCES_STILL_IN_THE_CODE}). A sentence written here is English for \
         everyone for ever: give it a `cli.*` or `ui.*` key in i18n/en.json and \
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
const USAGE: &str = "sailor worktree create <branch> [name]";
let _ = value.expect("the lock does not panic when nobody else holds it");
// a comment saying that no terminal has checked in yet is prose, not a sentence
"#;
    assert!(
        sentences_of(not_sentences).is_empty(),
        "something that is not a line for a person was counted: {:?}",
        sentences_of(not_sentences)
    );

    // The seam two earlier extractors went blind on: a line-shaped reader stops
    // at the margin and never sees the sentence that runs on past it.
    let across_the_margin = r#"
fn speak() -> String {
    "the quota of a PERSON and not of one run: it counts every session, \
     including the ones held outside"
        .to_owned()
}
"#;
    let over_there = sentences_of(across_the_margin);
    assert_eq!(
        over_there.len(),
        1,
        "a sentence that runs on past the margin was not seen: {over_there:?}"
    );
    assert!(
        !over_there[0].1.contains('\\'),
        "the sentence came back with the `\\` and the indent Rust throws away: {:?}",
        over_there[0].1
    );

    // The slots go, the description stays: a usage line that explains itself is
    // half prose, and the half a person reads has to be sayable in their own
    // language.
    let a_usage_line_that_explains_itself =
        "sailor worktree create <branch> [name]   opens a copy of the tree";
    assert!(
        is_a_sentence(a_usage_line_that_explains_itself),
        "the words a person actually reads were thrown out with the slots"
    );

    // A char literal holding a quote. Read as an opening string it swallows
    // everything down to the next quote — the doc comment included — and comes
    // back as a sentence, which is how four of them were counted in a file
    // whose prose was already all in the catalogue.
    let a_quote_that_is_a_char = r#"
fn probe(letter: char) -> bool {
    letter == '"'
}

/// a doc comment holding plenty of ordinary words for a reader to look at
const PROBE: &str = "ok";
"#;
    assert!(
        sentences_of(a_quote_that_is_a_char).is_empty(),
        "a char literal was read as a string, and what followed was counted: {:?}",
        sentences_of(a_quote_that_is_a_char)
    );

    // And a lifetime must still fall through to nothing: `'a` is not a char
    // literal, and swallowing it would hide the string that comes after.
    let a_lifetime_is_not_a_char = r#"
fn speak<'a>(from: &'a str) -> String {
    format!("no terminal has checked in yet, and none is expected: {from}")
}
"#;
    assert_eq!(
        sentences_of(a_lifetime_is_not_a_char).len(),
        1,
        "a lifetime was mistaken for a char literal and the sentence after it was lost"
    );

    let below_the_tests =
        "#[cfg(test)]\nmod tests {\n    let text = \"this one is only for a failing test\";\n}\n";
    assert!(
        sentences_of(below_the_tests).is_empty(),
        "the prose of a test was counted as a line for a person"
    );

    // **THE SHAPE IS NOT PROSE, AND THE FIELD IS WHAT SAYS SO — AND IT SAYS SO
    // ONLY THERE.** `form:` exempts what is typed; the sentence that travels
    // with it, in `says_key`, is a key and not prose either; and a sentence
    // written into any other field is still counted. Without this third half
    // the exemption would be a hole any literal could be pushed through by
    // renaming its field.
    let a_form_and_a_sentence_beside_it = r#"
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor terminal press --tty <name> --text <line> [--store <dir>]",
        says_key: "cli.terminal.form.press",
    },
    Form {
        form: "sailor terminal list",
        note: "types a line into a terminal that Sailor already holds open",
    },
];
"#;
    let counted = sentences_of(a_form_and_a_sentence_beside_it);
    assert_eq!(
        counted.len(),
        1,
        "the shape and its key must not count, and a sentence in any other field must: {counted:?}"
    );
    assert!(
        counted[0].1.starts_with("types a line"),
        "the one counted is not the sentence: {:?}",
        counted[0].1
    );
}
