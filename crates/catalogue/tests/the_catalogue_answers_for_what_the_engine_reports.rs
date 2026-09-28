//! Every failure the engine can report has a sentence, and every sentence has a
//! failure that can reach it. A class with no entry falls on `tryT(...) ??
//! failure` in `RunConsole.tsx`, so whoever hits it reads `subflow_too_deep`.
//! **AND A FAILURE HAS TWO DOORS**: `ActionError`, and the `closed(…)` the
//! executor uses when nothing raised an error — a process gone, an effect it
//! cannot read, a wait nobody took. Reading one left five classes mute.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The prefix the window looks a failure class up under. It is written here and
/// in `desktop/src/RunConsole.tsx`; if it ever moves, both move.
const FAILURE_PREFIX: &str = "run.failure.";

/// How many places build a failure whose class this scan cannot read, because it
/// is a variable rather than a literal. **The blind spot is declared, not
/// hidden**: no number at all would let it grow while the test stayed green.
///
/// **THE ONE LEFT CANNOT BE CLOSED**: `mcp.rs` takes the class from the tool
/// server's own status string, a word from outside this repository.
const CLASSES_THE_SCAN_CANNOT_READ_TODAY: usize = 1;

/// How far a seed may drift above what the tree actually holds. **Zero**, for
/// the reason written on the same constant in the comment ratchet: a seed is a
/// number in a file, and a merge taking the older side raises it with no
/// conflict and no signal.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

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
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// Every `src` file of every crate. **`src` and not `tests`**: a class a test
/// invents is not a class the engine can report, and counting it would make the
/// seed rise for work that changes nothing a user sees.
fn every_source() -> Vec<PathBuf> {
    every_source_under(&root())
}

/// The same reading, of whatever tree it is pointed at, so the verdict can be
/// put to a tree with a violation planted in it.
fn every_source_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        panic!("the crates directory is where this test looks and it is not there");
    };
    for entry in crates.flatten() {
        walk(&entry.path().join("src"), &mut found);
    }
    found
}

/// The word a call names its class with, once: a literal, or the value of a
/// `const` the tree declares. **A CLASS READ OFF A FAILURE ALREADY BUILT IS NOT
/// A NEW ONE**: it travels from the `ActionError` that carries it, and the scan
/// counted it where that was built. Anything else it cannot follow.
enum Named {
    Class(String),
    Relayed,
    Unreadable,
}

fn named(word: &str, constants: &BTreeMap<String, String>) -> Named {
    let word = word.trim();
    if let Some(class) = word.strip_prefix('"').and_then(|rest| rest.find('"').map(|end| rest[..end].to_owned())) {
        return Named::Class(class);
    }
    if let Some(class) = constants.get(word) {
        return Named::Class(class.clone());
    }
    if word.contains(".class") {
        return Named::Relayed;
    }
    Named::Unreadable
}

/// `const NAME: &str = "value";`, wherever the tree declares one. The executor
/// names the class of a lapsed wait that way, and a scan reading only literals
/// sees the constant's name and calls the class unreadable.
fn constants_in(text: &str, into: &mut BTreeMap<String, String>) {
    for line in text.lines() {
        let declared = line.trim_start();
        let declared = declared.strip_prefix("pub ").unwrap_or(declared);
        let declared = match declared.find(") ") {
            Some(end) if declared.starts_with("(") => &declared[end + 2..],
            _ => declared,
        };
        let Some(rest) = declared.strip_prefix("const ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": &str = ") else {
            continue;
        };
        if let Some(class) = value.strip_prefix('"').and_then(|rest| rest.find('"').map(|end| rest[..end].to_owned())) {
            into.insert(name.trim().to_owned(), class);
        }
    }
}

/// The arguments of a call, from the index of its opening bracket: split on the
/// commas at the top level, so a nested call or a literal holding one is whole.
fn arguments_at(text: &str, open: usize) -> Vec<String> {
    let mut depth = 0i32;
    let mut in_text = false;
    let mut found = Vec::new();
    let mut word = String::new();
    for letter in text[open..].chars() {
        if in_text {
            word.push(letter);
            if letter == '"' {
                in_text = false;
            }
            continue;
        }
        match letter {
            '"' => {
                in_text = true;
                word.push(letter);
            }
            '(' | '[' | '{' => {
                depth += 1;
                if depth > 1 {
                    word.push(letter);
                }
            }
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    found.push(word);
                    return found;
                }
                word.push(letter);
            }
            ',' if depth == 1 => found.push(std::mem::take(&mut word)),
            _ => word.push(letter),
        }
    }
    found
}

/// Where a name in the text opens a call of its own: `tree_closed(` is the tail
/// of another name, and `fn closed(` declares the one being called.
fn calls_here(text: &str, at: usize) -> bool {
    let before = &text[..at];
    before
        .chars()
        .next_back()
        .is_none_or(|letter| !letter.is_alphanumeric() && letter != '_')
        && !before.trim_end().ends_with("fn")
}

/// The class each place that builds a failure names, through either door.
fn classes_reported(text: &str, constants: &BTreeMap<String, String>) -> Vec<Named> {
    // The unit-test module at the foot of a file invents classes to exercise the
    // code around them. They are not classes the engine reports.
    let production = text.split("#[cfg(test)]").next().unwrap_or_default();
    let mut found = Vec::new();
    for (at, _) in production.match_indices("ActionError::new(") {
        let arguments = arguments_at(production, at + "ActionError::new".len());
        match arguments.first() {
            Some(class) => found.push(named(class, constants)),
            None => found.push(Named::Unreadable),
        }
    }
    // The executor closes a step with a class nobody raised: the fourth
    // argument, `None` when the step did not fail at all.
    for (at, _) in production.match_indices("closed(") {
        if !calls_here(production, at) {
            continue;
        }
        let arguments = arguments_at(production, at + "closed".len());
        let Some(class) = arguments.get(3) else {
            found.push(Named::Unreadable);
            continue;
        };
        let class = class.trim();
        if class == "None" {
            continue;
        }
        match class.strip_prefix("Some(").and_then(|rest| rest.strip_suffix(')')) {
            Some(inside) => found.push(named(inside, constants)),
            None => found.push(Named::Unreadable),
        }
    }
    found
}

fn measured() -> (Vec<String>, usize) {
    measured_under(&root())
}

fn measured_under(root: &Path) -> (Vec<String>, usize) {
    let sources = every_source_under(root);
    let read: Vec<String> = sources
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .collect();
    let mut constants = BTreeMap::new();
    for text in &read {
        constants_in(text, &mut constants);
    }
    let mut classes = Vec::new();
    let mut unreadable = 0;
    for text in &read {
        for reported in classes_reported(text, &constants) {
            match reported {
                Named::Class(class) => classes.push(class),
                Named::Relayed => {}
                Named::Unreadable => unreadable += 1,
            }
        }
    }
    classes.sort();
    classes.dedup();
    (classes, unreadable)
}

/// The absurd control. If the scan finds nothing, every count below is zero and
/// every assertion passes — a green test over a check that never ran.
#[test]
fn the_scan_finds_the_classes_that_are_known_to_be_there() {
    let (classes, _) = measured();
    // The last two come through `closed(…)` alone, one of them named by a
    // constant: a scan reading `ActionError` only finds neither.
    for known in [
        "engine_exit_error",
        "answer_not_json",
        "invalid_input",
        "process_disappeared",
        "handoff_expired",
    ] {
        assert!(
            classes.iter().any(|class| class == known),
            "the scan did not find «{known}», which is written out in the source: \
             every number this file reports is worthless until it does"
        );
    }
}

/// **A COVERAGE MEASURE IS CHECKED AGAINST A LIST THAT IS NOT ITS OWN.** A count
/// of what the walker found says nothing: one that quietly stopped opening a
/// whole kind of file still returns hundreds, and «hundreds» reads as «it looked
/// everywhere». So the list comes from git, and the demand is a name and not a
/// number: every source git tracks under a crate is one the walker opened.
#[test]
fn every_source_file_git_tracks_is_one_the_scan_opened() {
    // Fault 100: outside the top of a repository the oracle is empty, not clean.
    if !workspace::is_the_top_of_its_repository(&root()) {
        workspace::measured_nothing("this tree is not the top of a repository, so the list of tracked sources this coverage is checked against is empty");
        return;
    }
    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["ls-files", "--", "crates/*/src/*.rs", "crates/*/src/**/*.rs"])
        .output()
        .expect("git lists the files this check must cover, and it did not run");
    assert!(
        listed.status.success(),
        "git could not list the tracked sources, so this check has no oracle to \
         compare against and its silence would mean nothing"
    );

    let opened: Vec<PathBuf> = every_source();
    let tracked = std::str::from_utf8(&listed.stdout).unwrap_or_default().lines().count();
    workspace::measured_against(
        opened.len(),
        "sources opened under crates",
        tracked,
        "sources git tracks there",
    );
    let missed: Vec<&str> = std::str::from_utf8(&listed.stdout)
        .expect("git prints paths as utf-8")
        .lines()
        .filter(|tracked| !opened.iter().any(|path| path.ends_with(tracked)))
        .collect();

    assert!(
        missed.is_empty(),
        "git tracks {} sources under crates; the scan opened {} and never saw \
         these, so any class they report is uncounted and can go mute in \
         silence:\n{missed:#?}",
        std::str::from_utf8(&listed.stdout).unwrap_or_default().lines().count(),
        opened.len()
    );
}

/// **NO SEED HERE, BECAUSE THE DEBT IS PAID.** It stood at 38 for as long as it
/// took to write the entries; a class arriving without one is now a defect and
/// not a backlog, and gets refused as one.
/// The classes that reach the window with no sentence behind them.
fn mute_among(classes: &[String]) -> Vec<&String> {
    classes
        .iter()
        .filter(|class| catalogue::look("en", &format!("{FAILURE_PREFIX}{class}"), &[]).is_none())
        .collect()
}

#[test]
fn every_failure_the_engine_reports_has_a_sentence() {
    let (classes, _) = measured();
    let mute = mute_among(&classes);

    assert!(
        mute.is_empty(),
        "{} classes reach the window with no sentence, so whoever hits one reads \
         the bare class name. Write the entry in both i18n/en.json and \
         i18n/it.json — english is the source, so english is the one that must \
         be there:\n{mute:#?}",
        mute.len()
    );
}

#[test]
fn every_sentence_answers_for_a_failure_that_can_reach_it() {
    let (classes, _) = measured();
    let orphans: Vec<&str> = catalogue::every_key()
        .filter(|key| key.starts_with(FAILURE_PREFIX))
        .filter(|key| {
            let class = &key[FAILURE_PREFIX.len()..];
            !classes.iter().any(|reported| reported == class)
        })
        .collect();

    assert!(
        orphans.is_empty(),
        "these sentences answer for a class nothing reports any more; a renamed \
         class leaves its old sentence behind and the new one mute, which reads \
         as «the catalogue has an entry for that» right until someone hits it:\n\
         {orphans:#?}"
    );
}

#[test]
fn the_scans_blind_spot_does_not_grow() {
    let (_, unreadable) = measured();
    assert!(
        unreadable <= CLASSES_THE_SCAN_CANNOT_READ_TODAY,
        "{unreadable} places build a failure from a class this scan cannot read, \
         and the seed says {CLASSES_THE_SCAN_CANNOT_READ_TODAY}. Each one is a \
         class that could be missing its sentence with nothing to say so."
    );
    assert!(
        CLASSES_THE_SCAN_CANNOT_READ_TODAY.saturating_sub(unreadable) == HOW_STALE_A_SEED_MAY_BE,
        "the seed says {CLASSES_THE_SCAN_CANNOT_READ_TODAY} and the tree holds \
         {unreadable}: lower it"
    );
}

/// **THE VERDICT HAS ONLY EVER SEEN A TREE WITH NOTHING WRONG IN IT.** A mute
/// class and a class the scan cannot read are planted in a throwaway tree under
/// the temporary directory, through both doors and by both spellings, and the
/// same two verdicts are asked of it. A close that broke nothing and a close
/// relaying a failure already counted are planted beside them: neither is a
/// class of its own, and counting either would make every seed here drift.
#[test]
fn a_mute_class_planted_in_a_throwaway_tree_is_found() {
    let root = std::env::temp_dir().join(format!(
        "catalogue-planted-class-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let src = root.join("crates/sample/src");
    std::fs::create_dir_all(&src).expect("a throwaway crate directory");
    std::fs::write(
        src.join("lib.rs"),
        "const A_CLASS_IN_A_CONSTANT: &str = \"a_class_a_constant_names_with_no_sentence\";\n\
         fn fall(reason: &str) -> ActionError {\n    \
         ActionError::new(\"a_class_nobody_wrote_a_sentence_for\", reason)\n}\n\
         fn fall_again(class: &str) -> ActionError {\n    \
         ActionError::new(class, \"read off something outside this repository\")\n}\n\
         fn shut(now: i64) -> Completion {\n    \
         closed(Outcome::Broke, None, None, Some(\"a_class_a_close_hands_over_with_no_sentence\"), now)\n}\n\
         fn shut_by_constant(now: i64) -> Completion {\n    \
         closed(Outcome::Broke, None, None, Some(A_CLASS_IN_A_CONSTANT), now)\n}\n\
         fn shut_well(now: i64) -> Completion {\n    \
         closed(Outcome::Went, Some(json!({})), None, None, now)\n}\n\
         fn shut_relaying(error: ActionError, now: i64) -> Completion {\n    \
         closed(Outcome::Waiting, None, None, Some(error.class.as_str()), now)\n}\n",
    )
    .expect("the planted source writes");

    let (classes, unreadable) = measured_under(&root);
    let mute = mute_among(&classes);
    let named: Vec<String> = mute.into_iter().cloned().collect();
    std::fs::remove_dir_all(&root).expect("the throwaway tree goes");

    assert_eq!(
        named,
        vec![
            "a_class_a_close_hands_over_with_no_sentence".to_owned(),
            "a_class_a_constant_names_with_no_sentence".to_owned(),
            "a_class_nobody_wrote_a_sentence_for".to_owned(),
        ],
        "a class with no sentence was written into a source and the verdict did \
         not name it: whoever hit it would read the bare class name"
    );
    assert_eq!(
        unreadable, 1,
        "a failure built from a variable class was written into a source and the \
         blind-spot count did not rise, so the blind spot could grow unseen"
    );
}

/// **THE JUDGE MUST BE ABLE TO SAY IT DID NOT MEASURE.** The coverage above is
/// checked against what git tracks, and outside a repository that list is not
/// short: it is absent. A source with a reported class in it is planted so an
/// empty answer cannot pass for a covered tree — fault 100.
#[test]
fn a_tree_outside_a_repository_makes_the_coverage_declare_it_measured_nothing() {
    let plain = std::env::temp_dir()
        .join(format!("catalogue-uncovered-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&plain);
    let sources = plain.join("crates").join("a_crate").join("src");
    std::fs::create_dir_all(&sources).expect("a scratch");
    std::fs::write(sources.join("lib.rs"), "// a source no repository tracks\n")
        .expect("a source");

    assert!(
        !workspace::is_the_top_of_its_repository(&plain),
        "the road this judge takes to declare it measured nothing is closed"
    );
    assert_eq!(
        every_source_under(&plain).len(),
        1,
        "the walker did find the planted source, so the coverage would have had \
         something to compare — and nothing to compare it against"
    );

    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(&plain)
        .args(["ls-files", "--", "crates/*/src/*.rs"])
        .output()
        .expect("git runs");
    assert!(
        !listed.status.success(),
        "git listed a directory that is no repository, and an empty oracle read \
         as a covered tree is what fault 100 was"
    );

    let _ = std::fs::remove_dir_all(&plain);
}
