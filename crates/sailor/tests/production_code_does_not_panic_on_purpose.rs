//! Production code does not panic on purpose: the calls that end the program
//! instead of returning an error are counted per crate, and every count may
//! only fall.
//!
//! Test code is left out — `tests/`, `examples/`, `benches/`, and whatever
//! stands under `#[cfg(test)]` — because a test that panics is a test that fails.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The calls that abort instead of returning. `assert!` is not among them: an
/// assertion states an invariant, and another judge reads its prose.
const ON_PURPOSE: &[&str] = &[
    ".unwrap()",
    ".expect(",
    "panic!(",
    "unreachable!(",
    "todo!(",
    "unimplemented!(",
];

/// Per crate, as measured today; the shell under `desktop/` counts as one.
/// Downwards only: lowering a seed is the repair, raising one has to be
/// argued and shows in the diff.
const PANICS_TODAY: &[(&str, usize)] = &[
    ("actions", 3),
    ("catalogue", 1),
    ("desktop", 1),
    ("faults", 0),
    ("flow", 5),
    ("inventory", 0),
    ("ledger", 1),
    ("models", 4),
    ("profiles", 1),
    ("registry", 0),
    ("relay", 0),
    ("release", 0),
    ("sailor", 3),
    ("sessions", 0),
    ("supervisor", 0),
    ("terminal", 0),
    ("toolbox", 1),
    ("trigger", 1),
    ("ui", 0),
    ("workspace", 0),
];

/// How far a seed may sit above what the tree holds. Zero: a seed is a number
/// in a file, and a merge taking the older side raises it with no conflict.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

const MARKS_A_TEST: &str = "#[cfg(test)]";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// The crates the table has to name: every directory under `crates/`, and
/// the shell.
fn crates_of(root: &Path) -> Vec<String> {
    let mut names = vec!["desktop".to_owned()];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        names.extend(
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned()),
        );
    }
    names.sort();
    names
}

/// The crate a shipped file belongs to: the directory under `crates/`, and
/// `desktop` for the shell.
fn crate_of(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned());
    match parts.next()?.as_str() {
        "crates" => parts.next(),
        "desktop" => Some("desktop".to_owned()),
        _ => None,
    }
}

/// Every source file that ships: the `src` of each crate and of the shell,
/// without the directories that hold tests, examples and benches.
fn shipped_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        for package in crates.flatten() {
            walk(&package.path().join("src"), &mut found);
        }
    }
    walk(&root.join("desktop").join("src-tauri").join("src"), &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "tests" | "examples" | "benches" | "target") {
                walk(&path, found);
            }
        } else if name.ends_with(".rs") {
            found.push(path);
        }
    }
}

/// The text with every comment and every literal blanked, so a `.unwrap()`
/// quoted in prose or printed in a message is not a call, and a brace inside
/// a string does not open a block. Raw strings, escapes and the `'"'` char
/// literal are read the way the compiler reads them; lifetimes are left alone.
fn code_only(text: &str) -> String {
    let letters: Vec<char> = text.chars().collect();
    let mut kept = String::with_capacity(text.len());
    let mut at = 0usize;
    while at < letters.len() {
        let letter = letters[at];
        let next = letters.get(at + 1).copied();
        if letter == '/' && next == Some('/') {
            while at < letters.len() && letters[at] != '\n' {
                at += 1;
            }
            continue;
        }
        if letter == '/' && next == Some('*') {
            at = past_block_comment(&letters, at);
            kept.push(' ');
            continue;
        }
        if letter == 'r' && matches!(next, Some('"') | Some('#')) {
            if let Some(past) = past_raw_string(&letters, at) {
                kept.push(' ');
                at = past;
                continue;
            }
        }
        if letter == '"' {
            at = past_string(&letters, at);
            kept.push(' ');
            continue;
        }
        if letter == '\'' {
            if let Some(past) = past_char_literal(&letters, at) {
                kept.push(' ');
                at = past;
                continue;
            }
        }
        kept.push(letter);
        at += 1;
    }
    kept
}

/// Block comments nest, so `/* a /* b */ c */` closes on the second `*/`.
fn past_block_comment(letters: &[char], from: usize) -> usize {
    let mut at = from;
    let mut depth = 0usize;
    while at < letters.len() {
        if letters[at] == '/' && letters.get(at + 1) == Some(&'*') {
            depth += 1;
            at += 2;
        } else if letters[at] == '*' && letters.get(at + 1) == Some(&'/') {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    at
}

/// `r"…"` or `r#"…"#` with any run of hashes, closed by the same run.
fn past_raw_string(letters: &[char], from: usize) -> Option<usize> {
    let mut at = from + 1;
    let mut hashes = 0usize;
    while letters.get(at) == Some(&'#') {
        hashes += 1;
        at += 1;
    }
    if letters.get(at) != Some(&'"') {
        return None;
    }
    at += 1;
    while at < letters.len() {
        if letters[at] == '"' && letters[at + 1..].iter().take_while(|c| **c == '#').count() >= hashes {
            return Some(at + 1 + hashes);
        }
        at += 1;
    }
    Some(at)
}

fn past_string(letters: &[char], from: usize) -> usize {
    let mut at = from + 1;
    let mut escaped = false;
    while at < letters.len() {
        match letters[at] {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return at + 1,
            _ => {}
        }
        at += 1;
    }
    at
}

/// A char literal closes two letters on, or after its escape; a lifetime
/// never closes, and is not a literal.
fn past_char_literal(letters: &[char], from: usize) -> Option<usize> {
    let closes = if letters.get(from + 1) == Some(&'\\') {
        (from + 3..(from + 12).min(letters.len())).find(|&at| letters[at] == '\'')?
    } else {
        from + 2
    };
    (letters.get(closes) == Some(&'\'')).then_some(closes + 1)
}

/// The code without the items under `#[cfg(test)]`: a module with its body,
/// a single function, a `use`. A `mod name;` under it names a file, and the
/// name is returned so the caller can leave that file out too.
fn without_test_items(code: &str) -> (String, Vec<String>) {
    let mut kept = String::with_capacity(code.len());
    let mut elsewhere = Vec::new();
    let mut rest = code;
    while let Some(found) = rest.find(MARKS_A_TEST) {
        kept.push_str(&rest[..found]);
        let after = &rest[found + MARKS_A_TEST.len()..];
        let (header, consumed, has_no_body) = item_extent(after);
        if has_no_body {
            if let Some(name) = header
                .split_whitespace()
                .skip_while(|word| *word != "mod")
                .nth(1)
            {
                elsewhere.push(name.to_owned());
            }
        }
        rest = &after[consumed..];
    }
    kept.push_str(rest);
    (kept, elsewhere)
}

/// How far the item after an attribute runs: to its `;` when it has no body,
/// else to the brace that closes the first one it opens.
fn item_extent(after: &str) -> (String, usize, bool) {
    let mut header = String::new();
    let mut depth = 0usize;
    for (at, letter) in after.char_indices() {
        match (depth, letter) {
            (0, ';') => return (header, at + letter.len_utf8(), true),
            (0, '{') => depth = 1,
            (0, _) => header.push(letter),
            (_, '{') => depth += 1,
            (1, '}') => return (header, at + letter.len_utf8(), false),
            (_, '}') => depth -= 1,
            _ => {}
        }
    }
    (header, after.len(), false)
}

/// Where `mod name;` written in `file` puts its module: beside a `lib.rs`,
/// `main.rs` or `mod.rs`, under a directory named after any other file.
fn module_files(file: &Path, name: &str) -> [PathBuf; 2] {
    let dir = file.parent().unwrap_or_else(|| Path::new(""));
    let stem = file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let under = if matches!(stem.as_str(), "lib" | "main" | "mod") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    [under.join(format!("{name}.rs")), under.join(name).join("mod.rs")]
}

fn count_in(code: &str) -> usize {
    ON_PURPOSE.iter().map(|call| code.matches(call).count()).sum()
}

struct Measured {
    per_crate: BTreeMap<String, usize>,
    per_file: Vec<(usize, PathBuf)>,
}

fn measured() -> Measured {
    let root = root();
    let mut stripped: Vec<(PathBuf, String)> = Vec::new();
    let mut declared_as_tests: BTreeSet<PathBuf> = BTreeSet::new();
    for path in shipped_sources(&root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (code, elsewhere) = without_test_items(&code_only(&text));
        for name in elsewhere {
            declared_as_tests.extend(module_files(&path, &name));
        }
        stripped.push((path, code));
    }
    let mut per_crate: BTreeMap<String, usize> =
        crates_of(&root).into_iter().map(|name| (name, 0)).collect();
    let mut per_file = Vec::new();
    for (path, code) in stripped {
        if declared_as_tests.contains(&path) {
            continue;
        }
        let howmany = count_in(&code);
        if let Some(name) = crate_of(&root, &path) {
            *per_crate.entry(name).or_default() += howmany;
        }
        if howmany > 0 {
            per_file.push((howmany, path.strip_prefix(&root).unwrap_or(&path).to_path_buf()));
        }
    }
    per_file.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    Measured { per_crate, per_file }
}

fn table_of(per_crate: &BTreeMap<String, usize>) -> String {
    per_crate
        .iter()
        .map(|(name, howmany)| format!("    (\"{name}\", {howmany}),"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The twelve files holding most of what a red gate is asking about, so the
/// repair is a job and not a search.
fn heaviest(per_file: &[(usize, PathBuf)]) -> String {
    let mut said = String::from("\nWhere they are, heaviest first:");
    for (howmany, file) in per_file.iter().take(12) {
        said.push_str(&format!("\n  {howmany:>5}  {}", file.display()));
    }
    if per_file.len() > 12 {
        said.push_str(&format!("\n  … and {} more files", per_file.len() - 12));
    }
    said
}

/// Every crate's count is seeded exactly and may only fall: a crate that
/// panics more than its seed is red, and a seed left above the tree is a
/// seed nobody re-measured.
#[test]
fn no_crate_panics_on_purpose_more_than_today() {
    let found = measured();
    let seeded: BTreeMap<&str, usize> = PANICS_TODAY.iter().copied().collect();
    let mut complaints = Vec::new();
    for (name, howmany) in &found.per_crate {
        let seed = seeded.get(name.as_str()).copied();
        if !seed.is_some_and(|seed| *howmany <= seed) {
            complaints.push(format!(
                "crate «{name}» holds {howmany} calls that panic on purpose against a seed of {seed:?}: return an error instead, or the table is stale"
            ));
        } else if !seed.is_some_and(|seed| seed <= howmany + HOW_STALE_A_SEED_MAY_BE) {
            complaints.push(format!(
                "crate «{name}» is seeded at {seed:?} and holds {howmany}: lower the seed to {howmany}"
            ));
        }
    }
    assert!(
        complaints.is_empty(),
        "{}; measured now:\n{}{}",
        complaints.join("; "),
        table_of(&found.per_crate),
        heaviest(&found.per_file)
    );
    assert_eq!(
        seeded.len(),
        found.per_crate.len(),
        "the table names crates the tree lacks, or lacks some; measured now:\n{}",
        table_of(&found.per_crate)
    );
}

/// Whoever measures gets measured: were the scanner to stop seeing, or to see
/// tests, the counts would drift and every seed above would hold for ever.
#[test]
fn the_check_can_still_see_what_it_counts() {
    let fixture = r##"
fn shipped() -> u32 {
    let value = Some(1).unwrap();
    // a comment quoting .unwrap() is not a call
    /* nor a block one, /* nested */ with .expect( inside */
    let quoted = "a message that says .unwrap() and panic!( too";
    let raw = r"and a raw one with .expect(";
    let hashed = r#"and one with a brace { and .unwrap()"#;
    value
}

#[cfg(test)]
mod tests {
    #[test]
    fn inside() {
        let brace = "{";
        Some(2).unwrap();
        panic!("inside the module");
    }
}

fn after_the_module() {
    Option::<u32>::None.expect("counted");
}

#[cfg(test)]
fn helper() {
    unreachable!("not counted");
}

#[cfg(not(test))]
fn shipped_too() {
    todo!("counted")
}

fn quote<'a>(text: &'a str) -> char {
    let quote = '"';
    let escaped = '\'';
    let _ = Some(quote).unwrap();
    let _ = text.chars().next().unwrap();
    escaped
}
"##;
    let (code, elsewhere) = without_test_items(&code_only(fixture));
    assert_eq!(
        count_in(&code),
        5,
        "one call in `shipped`, one after the module, one under `cfg(not(test))`, two in `quote`; got:\n{code}"
    );
    assert!(elsewhere.is_empty(), "no module of the fixture lives in another file");
    assert!(!code.contains("inside the module"), "the test module is gone");
    assert!(code.contains("after_the_module"), "the code after it is not");
    assert!(!code.contains("unreachable!("), "a single function under cfg(test) is gone too");
    assert!(code.contains("'a"), "a lifetime is not a char literal");

    let every = "x.unwrap(); x.expect(\"a\"); panic!(\"b\"); unreachable!(); todo!(); unimplemented!();";
    assert_eq!(count_in(&code_only(every)), ON_PURPOSE.len(), "each call is seen once");
    assert_eq!(count_in(&code_only("x.unwrap_or_default(); x.expect_err(\"a\");")), 0, "the cousins are not");

    let (code, elsewhere) = without_test_items(&code_only(
        "#[cfg(test)]\nmod tests;\nfn shipped() { x.unwrap(); }",
    ));
    assert_eq!(elsewhere, vec!["tests"], "a module without a body names a file");
    assert_eq!(count_in(&code), 1, "and the code around it is still counted");
    assert_eq!(
        module_files(Path::new("crates/x/src/lib.rs"), "tests"),
        [PathBuf::from("crates/x/src/tests.rs"), PathBuf::from("crates/x/src/tests/mod.rs")]
    );
    assert_eq!(
        module_files(Path::new("crates/x/src/store.rs"), "tests")[0],
        PathBuf::from("crates/x/src/store/tests.rs")
    );

    let found = measured();
    println!("today, per crate:\n{}{}", table_of(&found.per_crate), heaviest(&found.per_file));
    assert!(
        found.per_crate.values().sum::<usize>() > 0,
        "zero calls in the whole tree: the counter is not looking"
    );
    let root = root();
    assert!(
        !shipped_sources(&root).iter().any(|path| path.components().any(|part| part.as_os_str() == "tests")),
        "a `tests` directory is never a shipped source"
    );
}
