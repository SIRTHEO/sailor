//! Every check a document declares names a test that exists.
//!
//! A rule came in with its check written only as prose, and a test nobody
//! wrote never fails: the page that promises it stays green by definition.
//! So every section under `docs/` that declares its own check is read here,
//! and the tree is asked for the test it names — see fault 67.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Sections that declare a check and name no test, as counted today.
/// Downwards only: each is a rule still waiting for what makes it red, and
/// the red message lists them by file and line so somebody can write it.
const PROSE_ONLY_CHECKS_TODAY: usize = 37;

/// How far the seed may sit above what the docs hold. Zero: a seed nobody
/// re-measured buys silence for work nobody did.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// A heading opening with one of these declares the check of its page.
const HEADINGS_THAT_DECLARE_A_CHECK: &[&str] = &[
    "Il controllo",
    "La prova",
    "Come si controlla",
    "Cosa lo rende rosso",
    "The measure",
    "The check",
    "How it is checked",
    "What makes it red",
    "Proof",
];

/// An emphasised lead opening the check of one claim, as in `*Proof:*`.
const LEADS_THAT_DECLARE_A_CHECK: &[&str] = &["Proof:", "Proof.", "Prova:", "Prova."];

/// The command form: the word after this flag is a test target outright.
const TEST_TARGET_FLAG: &str = "--test";

/// Where a test may live: the crates, and the window's own Rust.
const RUST_SOURCES: &[&str] = &["crates", "desktop/src-tauri/src"];

/// This tree names its tests as sentences. A backticked word with fewer
/// underscores is a field, a flag or a module, not a test.
const UNDERSCORES_IN_A_TEST_NAME: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Heading,
    Lead,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Named {
    /// A test function, or the stem of a test file.
    Test(String),
    /// A test target as the test command takes it: the stem of a test file.
    Target(String),
    /// A file under a crate's `tests`, by its path from the root.
    File(String),
}

impl Named {
    fn as_written(&self) -> &str {
        match self {
            Named::Test(name) | Named::Target(name) | Named::File(name) => name,
        }
    }
}

#[derive(Debug)]
struct Declared {
    file: PathBuf,
    line: usize,
    form: Form,
    names: Vec<Named>,
}

impl Declared {
    fn where_it_is(&self) -> String {
        format!("{}:{}", self.file.display(), self.line)
    }
}

/// What the tree holds: every test function, and every file under a
/// crate's `tests` by its path from the root.
#[derive(Debug, Default)]
struct Tree {
    test_functions: BTreeSet<String>,
    test_files: BTreeSet<String>,
}

impl Tree {
    fn read(root: &Path) -> Tree {
        let mut tree = Tree::default();
        let mut sources = Vec::new();
        for place in RUST_SOURCES {
            rust_files_under(&root.join(place), &mut sources);
        }
        for path in sources {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            tree.test_functions.extend(test_functions_in(&text));
            let relative = path.strip_prefix(root).ok().and_then(Path::to_str);
            if let Some(relative) = relative.filter(|relative| is_a_test_file(relative)) {
                tree.test_files.insert(relative.to_owned());
            }
        }
        tree
    }

    fn has(&self, named: &Named) -> bool {
        match named {
            Named::File(path) => self.test_files.contains(path),
            Named::Target(stem) => self.has_a_file_called(stem),
            Named::Test(name) => self.test_functions.contains(name) || self.has_a_file_called(name),
        }
    }

    fn has_a_file_called(&self, stem: &str) -> bool {
        let tail = format!("/tests/{stem}.rs");
        self.test_files.iter().any(|file| file.ends_with(&tail))
    }
}

fn rust_files_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "node_modules" {
                rust_files_under(&path, found);
            }
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// `crates/<crate>/tests/<name>.rs`, and nothing deeper: a file below that
/// is a helper module, not a target the test command can run.
fn is_a_test_file(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    parts.len() == 4 && parts[0] == "crates" && parts[2] == "tests" && parts[3].ends_with(".rs")
}

/// The functions under a test attribute, looking past any attribute between.
fn test_functions_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut under_a_test_attribute = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[test") {
            under_a_test_attribute = true;
        } else if under_a_test_attribute && !trimmed.starts_with("#[") {
            found.extend(function_name(trimmed));
            under_a_test_attribute = false;
        }
    }
    found
}

fn function_name(line: &str) -> Option<String> {
    let after_fn = line.find("fn ")? + "fn ".len();
    let name: String = line[after_fn..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The tests a stretch of text names, in the three shapes the docs use: a
/// path under a crate's `tests`, a backticked sentence-name, and the target
/// after the flag of the test command.
fn names_in(text: &str) -> Vec<Named> {
    let mut found: Vec<Named> = paths_in(text).into_iter().map(Named::File).collect();
    found.extend(backticked_words_in(text).into_iter().map(Named::Test));
    found.extend(targets_in(text).into_iter().map(Named::Target));
    found.sort();
    found.dedup();
    found
}

fn paths_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("crates/") {
        let candidate: String = rest[at..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '.'))
            .collect();
        let candidate = candidate.trim_end_matches('.');
        if is_a_test_file(candidate) {
            found.push(candidate.to_owned());
        }
        rest = &rest[at + "crates/".len()..];
    }
    found
}

/// The spans between paired backticks; an unpaired one closes nothing.
fn backticked_words_in(text: &str) -> Vec<String> {
    let pieces: Vec<&str> = text.split('`').collect();
    (1..pieces.len().saturating_sub(1))
        .step_by(2)
        .filter_map(|index| test_name_from(pieces[index]))
        .collect()
}

/// A backticked span names a test when, shed of a line reference, a call's
/// parentheses or a file suffix, it is a lowercase identifier long enough
/// to be a sentence.
fn test_name_from(span: &str) -> Option<String> {
    let word = span.trim().split(':').next()?;
    let word = word.strip_suffix("()").unwrap_or(word);
    let word = word.strip_suffix(".rs").unwrap_or(word);
    let is_identifier = word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    (is_identifier && word.matches('_').count() >= UNDERSCORES_IN_A_TEST_NAME)
        .then(|| word.to_owned())
}

/// The word after the test flag, when the flag stands alone: a longer flag
/// that merely begins the same way names nothing.
fn targets_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(TEST_TARGET_FLAG) {
        rest = &rest[at + TEST_TARGET_FLAG.len()..];
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let target: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !target.is_empty() {
            found.push(target);
        }
    }
    found
}

/// The text of a heading, with its marks and any numbering shed.
fn heading_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let text = trimmed.trim_start_matches('#');
    if text.len() == trimmed.len() || !text.starts_with(' ') {
        return None;
    }
    let numbering = |c: char| c.is_ascii_digit() || c == '.';
    Some(text.trim_start().trim_start_matches(numbering).trim_start())
}

fn declares_a_check_by_heading(text: &str) -> bool {
    HEADINGS_THAT_DECLARE_A_CHECK.iter().any(|phrase| {
        text.strip_prefix(phrase)
            .is_some_and(|rest| !rest.starts_with(char::is_alphabetic))
    })
}

/// The leads on a line that declare a check, in the same shape.
fn check_leads_on(line: &str) -> Vec<(usize, &str, usize)> {
    leads_on(line)
        .into_iter()
        .filter(|(_, inner, _)| LEADS_THAT_DECLARE_A_CHECK.contains(&inner.trim()))
        .collect()
}

/// Emphasised leads on a line — `*Word …:*` or `*Word ….*` — as the column
/// of the opening star, the text inside, and the column of the closing star.
fn leads_on(line: &str) -> Vec<(usize, &str, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = line[from..].find('*').map(|at| at + from) {
        let after = open + 1;
        from = after;
        if !line[after..].starts_with(char::is_uppercase) {
            continue;
        }
        let Some(close) = line[after..].find('*').map(|at| at + after) else {
            break;
        };
        let inner = &line[after..close];
        if inner.ends_with(':') || inner.ends_with('.') {
            found.push((open, inner, close));
        }
    }
    found
}

fn starts_a_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    let after_marker = match trimmed.strip_prefix(['-', '*', '+']) {
        Some(rest) => Some(rest),
        None => {
            let digits = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
            (digits.len() < trimmed.len())
                .then(|| digits.strip_prefix(['.', ')']))
                .flatten()
        }
    };
    after_marker.is_some_and(|rest| rest.starts_with(' '))
}

fn is_a_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Which lines are prose rather than fenced code: a `#` in a shell block is
/// not a heading and does not close one.
fn structural_lines(lines: &[&str]) -> Vec<bool> {
    let mut inside_a_fence = false;
    lines
        .iter()
        .map(|line| {
            if is_a_fence(line) {
                inside_a_fence = !inside_a_fence;
                return false;
            }
            !inside_a_fence
        })
        .collect()
}

fn is_a_heading(lines: &[&str], structural: &[bool], index: usize) -> bool {
    structural[index] && heading_text(lines[index]).is_some()
}

/// A lead's section: the rest of its line and the lines after, up to the
/// next lead, a blank line, a heading or a list item.
fn lead_section(lines: &[&str], structural: &[bool], index: usize, from: usize) -> (String, usize) {
    let mut text = String::new();
    let mut end = index + 1;
    let first = &lines[index][from..];
    if let Some((column, _, _)) = leads_on(first).first() {
        text.push_str(&first[..*column]);
        return (text, end);
    }
    text.push_str(first);
    while end < lines.len() {
        let line = lines[end];
        let closes = line.trim().is_empty()
            || is_a_heading(lines, structural, end)
            || (structural[end] && starts_a_list_item(line));
        if closes {
            break;
        }
        text.push(' ');
        end += 1;
        if let Some((column, _, _)) = leads_on(line).first() {
            text.push_str(&line[..*column]);
            break;
        }
        text.push_str(line);
    }
    (text, end)
}

/// The paragraph around a line: the run of lines with text on them that it
/// sits in, bounded by headings and fences.
fn paragraph_around(lines: &[&str], structural: &[bool], index: usize) -> (usize, usize) {
    let bounds = |at: usize| {
        lines[at].trim().is_empty() || is_a_fence(lines[at]) || is_a_heading(lines, structural, at)
    };
    let mut start = index;
    while start > 0 && !bounds(start - 1) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < lines.len() && !bounds(end) {
        end += 1;
    }
    (start, end)
}

/// A heading's section: its prose up to the next heading, or up to the
/// first lead that declares a check, whose names are the lead's own.
fn heading_section(lines: &[&str], structural: &[bool], index: usize) -> (String, usize) {
    let mut text = String::new();
    let mut end = index;
    while end < lines.len() && (end == index || !is_a_heading(lines, structural, end)) {
        let line = lines[end];
        if let Some((column, _, _)) = check_leads_on(line).first().filter(|_| structural[end]) {
            text.push_str(&line[..*column]);
            return (text, end);
        }
        text.push_str(line);
        text.push(' ');
        end += 1;
    }
    (text, end)
}

/// The sections of one document that declare a check, each with the tests
/// it names. A paragraph running the test command counts only outside the
/// heading and lead sections already found, so no section is read twice.
fn declared_in(file: &Path, text: &str) -> Vec<Declared> {
    let lines: Vec<&str> = text.lines().collect();
    let structural = structural_lines(&lines);
    let mut covered = vec![false; lines.len()];
    let mut found = Vec::new();
    let mut declare = |line: usize, form: Form, names: Vec<Named>| {
        found.push(Declared {
            file: file.to_path_buf(),
            line: line + 1,
            form,
            names,
        });
    };

    for (index, line) in lines.iter().enumerate() {
        if !structural[index] {
            continue;
        }
        if heading_text(line).is_some_and(declares_a_check_by_heading) {
            let (text, end) = heading_section(&lines, &structural, index);
            covered[index..end].fill(true);
            declare(index, Form::Heading, names_in(&text));
        }
        for (_, _, close) in check_leads_on(line) {
            let (text, end) = lead_section(&lines, &structural, index, close + 1);
            covered[index..end].fill(true);
            declare(index, Form::Lead, names_in(&text));
        }
    }
    for index in 0..lines.len() {
        let with_the_next = lines[index..lines.len().min(index + 2)].join(" ");
        if covered[index] || targets_in(&with_the_next).is_empty() {
            continue;
        }
        let (start, end) = paragraph_around(&lines, &structural, index);
        covered[start..end].fill(true);
        declare(index, Form::Command, names_in(&lines[start..end].join(" ")));
    }
    found.sort_by_key(|declared| declared.line);
    found
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root")
}

fn documents_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            documents_under(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "md") {
            found.push(path);
        }
    }
}

fn declared_in_the_docs(root: &Path) -> Vec<Declared> {
    let mut documents = Vec::new();
    documents_under(&root.join("docs"), &mut documents);
    documents.sort();
    documents
        .iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let relative = path.strip_prefix(root).unwrap_or(path);
            Some(declared_in(relative, &text))
        })
        .flatten()
        .collect()
}

/// Every name a section gives that the tree does not have, as `file:line: name`.
fn missing(declared: &[Declared], tree: &Tree) -> Vec<String> {
    declared
        .iter()
        .flat_map(|section| {
            section
                .names
                .iter()
                .filter(|named| !tree.has(named))
                .map(move |named| format!("{}: {}", section.where_it_is(), named.as_written()))
        })
        .collect()
}

/// Every section that declares a check and names nothing, as `file:line`.
fn prose_only(declared: &[Declared]) -> Vec<String> {
    declared
        .iter()
        .filter(|section| section.names.is_empty())
        .map(Declared::where_it_is)
        .collect()
}

#[test]
fn every_test_a_document_names_exists_in_the_tree() {
    let root = root();
    let missing = missing(&declared_in_the_docs(&root), &Tree::read(&root));
    assert!(
        missing.is_empty(),
        "a document declares its check by naming a test the tree does not have. \
         Write that test, or name the one that exists:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_checks_declared_in_prose_only_ever_fall() {
    let prose_only = prose_only(&declared_in_the_docs(&root()));
    assert!(
        prose_only.len() <= PROSE_ONLY_CHECKS_TODAY,
        "sections that declare a check without naming a test: {} (the seed is \
         {PROSE_ONLY_CHECKS_TODAY}). A check written in prose never fails: name \
         the test that makes the rule red, and write it if it does not exist. \
         The seed does not rise.\n  {}",
        prose_only.len(),
        prose_only.join("\n  ")
    );
}

#[test]
fn a_seed_that_no_longer_describes_the_docs_is_a_seed_nobody_re_measured() {
    let measured = prose_only(&declared_in_the_docs(&root())).len();
    assert!(
        PROSE_ONLY_CHECKS_TODAY <= measured + HOW_STALE_A_SEED_MAY_BE,
        "the seed says {PROSE_ONLY_CHECKS_TODAY} and the docs hold {measured}: \
         lower the seed to what was measured, so the next prose-only check is caught"
    );
}

/// A count that stopped counting reads as agreement. Every form is still
/// written somewhere, the tree still has tests, and names were found.
#[test]
fn the_scanner_can_still_see_what_it_reads() {
    let root = root();
    let declared = declared_in_the_docs(&root);
    let of = |form: Form| {
        declared
            .iter()
            .filter(|section| section.form == form)
            .count()
    };
    let forms = (of(Form::Heading), of(Form::Lead), of(Form::Command));
    assert!(
        forms.0 >= 1 && forms.1 >= 10 && forms.2 >= 3,
        "declaring sections found by form (heading, lead, command): {forms:?}; \
         a form at zero means the docs changed shape or the scanner stopped seeing it"
    );
    let named: usize = declared.iter().map(|section| section.names.len()).sum();
    assert!(
        named >= 20,
        "only {named} test names were found across every declaring section"
    );

    let tree = Tree::read(&root);
    assert!(
        tree.test_functions.len() >= 1_000 && tree.test_files.len() >= 100,
        "the tree read {} test functions and {} test files, so almost nothing could be found",
        tree.test_functions.len(),
        tree.test_files.len()
    );
}

fn fixture(text: &str) -> Vec<Declared> {
    declared_in(Path::new("docs/fixture.md"), text)
}

fn tree_with(functions: &[&str], files: &[&str]) -> Tree {
    Tree {
        test_functions: functions.iter().map(|name| (*name).to_owned()).collect(),
        test_files: files.iter().map(|path| (*path).to_owned()).collect(),
    }
}

#[test]
fn a_section_naming_a_test_that_exists_passes() {
    let declared = fixture(
        "## Il controllo che rende rossa questa pagina\n\n\
         Una prova scorre i descrittori: `a_descriptor_without_a_limit_is_refused`.\n",
    );
    let tree = tree_with(&["a_descriptor_without_a_limit_is_refused"], &[]);
    assert_eq!(declared.len(), 1, "{declared:?}");
    assert_eq!(declared[0].form, Form::Heading);
    assert!(
        missing(&declared, &tree).is_empty(),
        "{:?}",
        missing(&declared, &tree)
    );
    assert!(
        prose_only(&declared).is_empty(),
        "{:?}",
        prose_only(&declared)
    );
}

#[test]
fn a_section_naming_a_missing_test_is_reported_with_file_and_line() {
    let declared = fixture(
        "1. **A claim.** Some words about it.\n   \
         *Proof:* `a_test_nobody_ever_wrote` in `crates/nowhere/tests/never_written.rs`.\n",
    );
    let tree = tree_with(
        &["some_other_test_that_exists"],
        &["crates/sailor/tests/real.rs"],
    );
    assert_eq!(
        missing(&declared, &tree),
        vec![
            "docs/fixture.md:2: a_test_nobody_ever_wrote".to_owned(),
            "docs/fixture.md:2: crates/nowhere/tests/never_written.rs".to_owned(),
        ]
    );
    assert!(
        prose_only(&declared).is_empty(),
        "a section that names tests is not prose-only"
    );
}

#[test]
fn a_section_that_declares_a_check_in_prose_only_is_counted() {
    let declared = fixture(
        "## The measure\n\n\
         One working day inside the tool, without leaving it.\n\n\
         ## Il controllo che rende rossa questa pagina\n\n\
         Una prova scorre i descrittori e fallisce se uno non dichiara il limite.\n\
         Nasce verde oggi.\n\n\
         ## Another heading\n\nNothing declared here.\n",
    );
    assert_eq!(
        prose_only(&declared),
        vec![
            "docs/fixture.md:1".to_owned(),
            "docs/fixture.md:5".to_owned()
        ]
    );
    assert!(
        missing(&declared, &tree_with(&[], &[])).is_empty(),
        "prose names nothing to miss"
    );
}

#[test]
fn each_form_of_declaration_is_recognised_with_each_shape_of_name() {
    let declared = fixture(
        "The measure is `cargo test -p sailor --test the_fault_table_holds_together`,\n\
         whose count only falls.\n\n\
         ## La prova che va rossa per prima\n\n\
         La prova `an_engine_step_declares_what_it_can_return_and_what_it_hands_on`\n\
         (`crates/sailor/tests/dispatch_the_work.rs:229-291`) already asks it; a field\n\
         like `answer_not_json` is not a test.\n\n\
         1. **A claim.** *Proof:* `the_seed_is_read_from_the_file.rs`; mutant red.\n   \
         *Where it landed:* `a_name_in_the_next_lead_is_not_this_proofs`.\n\
         2. **Another claim.** *Proof:* `cargo test -p sailor --test\n   \
         identifiers_are_in_english` is red on one; `sailor flow check` is not a test.\n",
    );
    let forms: Vec<(usize, Form)> = declared
        .iter()
        .map(|section| (section.line, section.form))
        .collect();
    assert_eq!(
        forms,
        vec![
            (1, Form::Command),
            (4, Form::Heading),
            (10, Form::Lead),
            (12, Form::Lead)
        ]
    );
    assert_eq!(
        declared[0].names,
        vec![Named::Target("the_fault_table_holds_together".to_owned())]
    );
    assert_eq!(
        declared[1].names,
        vec![
            Named::Test(
                "an_engine_step_declares_what_it_can_return_and_what_it_hands_on".to_owned()
            ),
            Named::File("crates/sailor/tests/dispatch_the_work.rs".to_owned()),
        ]
    );
    assert_eq!(
        declared[2].names,
        vec![Named::Test("the_seed_is_read_from_the_file".to_owned())]
    );
    assert_eq!(
        declared[3].names,
        vec![Named::Target("identifiers_are_in_english".to_owned())]
    );

    let tree = tree_with(
        &["an_engine_step_declares_what_it_can_return_and_what_it_hands_on"],
        &[
            "crates/sailor/tests/the_fault_table_holds_together.rs",
            "crates/sailor/tests/dispatch_the_work.rs",
            "crates/other/tests/the_seed_is_read_from_the_file.rs",
            "crates/sailor/tests/identifiers_are_in_english.rs",
        ],
    );
    assert!(
        missing(&declared, &tree).is_empty(),
        "{:?}",
        missing(&declared, &tree)
    );
}

#[test]
fn a_heading_inside_a_fence_is_code_and_a_longer_flag_names_nothing() {
    let declared = fixture(
        "```sh\n# The measure\ncargo test --tests --test-threads=1\n```\n\n\
         Plain prose with `--tests` and `--test-threads=1` in it.\n",
    );
    assert!(declared.is_empty(), "{declared:?}");
}

#[test]
fn a_test_function_is_found_under_its_attribute_even_past_another() {
    let found = test_functions_in(
        "#[test]\n#[ignore]\nfn a_b_c() {}\n    #[test]\n    fn d_e_f() {}\nfn not_a_test() {}\n\
         /// #[test] in a comment\nfn g_h_i() {}\n",
    );
    assert_eq!(found, vec!["a_b_c".to_owned(), "d_e_f".to_owned()]);
}
