//! What the program says is in English, and the number only falls.
//!
//! Fault 124: the comment ratchet drove its own debt from 6,947 lines to one
//! and the sentences the program prints stayed Italian, because a measurement
//! improves only what it can see. This counts the other half.

use std::path::{Path, PathBuf};

/// How many string literals still carry the other language.
///
/// **THE ONLY HONEST RAISE** is a merge bringing in sentences written
/// elsewhere: re-measure, raise to the measured number, and say so in the
/// commit. Raising it because it went red is disarming it.
const SENTENCES_NOT_IN_ENGLISH: usize = 0;

/// How far the seed may sit above the tree. **Zero**, for the reason the
/// sibling ratchets give: a seed is a number in a file, and a file merges.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// Words no English sentence uses. The same list the comment judge carries,
/// because the two count the same language in two places.
const WORDS_NO_ENGLISH_SENTENCE_USES: &[&str] = &[
    "che", "non", "della", "delle", "degli", "nella", "nelle", "questo", "questa", "quello",
    "quella", "perché", "perche", "cioè", "cioe", "invece", "quindi", "anche", "essere", "senza",
    "più", "piu", "già", "gia", "sono", "dove", "quando", "sulla", "dalla", "dello", "il", "lo",
    "le", "gli", "un", "una", "nel", "dei", "alla", "allo", "alle", "sul", "sui", "dal", "dalle",
    "questi", "queste", "quali", "quale", "ogni", "solo", "ancora", "adesso", "prima", "dopo",
];

/// **THE COUNT ERRS DOWNWARDS, ON PURPOSE**, and twice over: a literal using
/// only words the two languages share is never accused, and a hyphenated
/// fixture id splits into words that are. So this is a floor, and an English
/// sentence is never accused — which would make the debt unpayable.
fn is_not_english(text: &str) -> bool {
    let lowered = text.to_lowercase();
    lowered
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|word| word.len() > 1)
        .any(|word| WORDS_NO_ENGLISH_SENTENCE_USES.contains(&word))
}

/// **THE PRICE, DECLARED.** This reads text, not a syntax tree: a literal split
/// across lines is seen a line at a time, and a comment that looks like a
/// string is skipped by the crudest rule there is. It finds sentences, which is
/// what it is for; it does not claim to find every one.
fn sentences_in(line: &str) -> Vec<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
        return Vec::new();
    }
    let mut found = Vec::new();
    for quote in ['"', '\''] {
        let mut rest = line;
        while let Some(open) = rest.find(quote) {
            let after = &rest[open + 1..];
            let Some(close) = after.find(quote) else { break };
            found.push(after[..close].to_owned());
            rest = &after[close + 1..];
        }
    }
    found
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                walk(&path, found);
            }
        } else if name != "the_words_a_user_reads_are_in_english.rs"
            && [".rs", ".ts", ".tsx"].iter().any(|end| name.ends_with(end))
        {
            found.push(path);
        }
    }
}

/// Where the sentences a person can read are written.
///
/// **THE WHOLE OF `crates` WOULD COUNT FIXTURES**, which are data: an invented
/// flow id is not language. Only what the product says of itself is measured —
/// the command line, the shared view, the window's shell, and the three crates
/// that answer a person while a terminal is open.
const WHERE_THE_PRODUCT_SPEAKS: &[&str] = &[
    "crates/sailor/src",
    "crates/ui/src",
    "crates/actions/src",
    "crates/toolbox/src",
    "crates/terminal/src",
    "crates/supervisor/src",
    "crates/relay/src",
    "desktop/src-tauri/src",
];

struct Found {
    count: usize,
    /// How many sources were opened. **THE RECEIPT IS THE PERIMETER, NOT THE
    /// DEBT**: handed the debt, this judge reported «measured: 0» the day the
    /// debt reached zero, and `sailor ratchet` read that as a judge that had
    /// walked nothing — green, and saying nothing about the tree.
    walked: usize,
    worst: Vec<String>,
}

fn count_in(root: &Path) -> Found {
    let mut sources = Vec::new();
    for place in WHERE_THE_PRODUCT_SPEAKS {
        let before = sources.len();
        walk(&root.join(place), &mut sources);
        // **A TREE THAT MOVED TAKES THE PERIMETER WITH IT.** Renamed, `walk`
        // returns nothing, the debt is nothing, and a seed of nothing agrees:
        // the judge would pass by having stopped looking.
        assert!(
            sources.len() > before,
            "«{place}» holds no source this judge can read: it was renamed, or the list is stale"
        );
    }
    let walked = sources.len();
    let (mut count, mut worst) = (0, Vec::new());
    for path in sources {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut inside_tests = false;
        for line in text.lines() {
            // A test's own prose is read by whoever debugs it, never printed.
            if line.trim_start().starts_with("#[cfg(test)]") {
                inside_tests = true;
            }
            if inside_tests {
                continue;
            }
            for said in sentences_in(line) {
                if is_not_english(&said) {
                    count += 1;
                    if worst.len() < 10 {
                        worst.push(format!("{}: {said}", path.display()));
                    }
                }
            }
        }
    }
    Found { count, walked, worst }
}

/// **THE SEED IS A KNOB, NOT A LITERAL**, and clippy cannot know that: at zero
/// it reads `count <= 0` as a comparison against the minimum of a `usize` and
/// refuses to compile. Widening both sides keeps the two assertions written as
/// every sibling ratchet writes them, so raising the seed one day changes a
/// number and nothing else.
#[test]
fn the_sentences_the_product_says_only_ever_get_more_english() {
    let declared = SENTENCES_NOT_IN_ENGLISH as i64;
    let stale = HOW_STALE_A_SEED_MAY_BE as i64;
    let found = count_in(&root());
    let measured = found.count as i64;
    workspace::measured_against(
        found.walked,
        "sources the product speaks from",
        WHERE_THE_PRODUCT_SPEAKS.len(),
        "trees it was pointed at",
    );
    assert!(
        measured <= declared,
        "sentences not in English: {} (declared {SENTENCES_NOT_IN_ENGLISH}). \
         Write the new one in English; if you are translating, lower the number.\n{}",
        found.count,
        found.worst.join("\n")
    );
    assert!(
        declared <= measured + stale,
        "the seed says {SENTENCES_NOT_IN_ENGLISH}, the tree holds {}: write the measured number",
        found.count
    );
}

/// Whoever measures gets measured: a counter that stopped seeing would let the
/// number fall to zero and the test above would only ask whether the seed had
/// followed it down.
#[test]
fn the_counter_can_still_see_what_it_counts() {
    assert!(is_not_english("il flusso non esiste"));
    assert!(
        !is_not_english("the flow does not exist"),
        "an English sentence must never be accused, or the debt could not be paid by translating"
    );
    assert_eq!(
        sentences_in(r#"    let said = "una frase";"#),
        vec!["una frase".to_owned()],
        "the reader must find a literal on a line of code"
    );
    assert!(
        sentences_in(r#"    // "una frase" in a comment is not one"#).is_empty(),
        "a comment is the other ratchet's business"
    );
}
