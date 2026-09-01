//! Every failure the engine can report has a sentence, and every sentence has a
//! failure that can reach it.
//!
//! **THE ENGINE AND THE CATALOGUE ARE TWO HAND-WRITTEN LISTS OVER ONE CLOSED
//! SET, AND NOTHING COMPARED THEM.** A class added in Rust with no entry here
//! reaches `RunConsole.tsx` and falls on `tryT(...) ?? failure`, so the person
//! who hit it reads `subflow_too_deep` where a sentence should be. Nothing goes
//! red; the window renders it and looks like it worked.
//!
//! The check reads the source rather than a registry because the source is where
//! the classes actually are. When the classes become a closed type this file
//! gets shorter, not obsolete: the pairing it guards is the point.

use std::path::{Path, PathBuf};

/// The prefix the window looks a failure class up under. It is written here and
/// in `desktop/src/RunConsole.tsx`; if it ever moves, both move.
const FAILURE_PREFIX: &str = "run.failure.";

/// How many classes the engine can report with no sentence to show for them.
///
/// **It can only go down.** Lowering it is the repair — write the two entries,
/// English and Italian — and raising it means a class was added without one,
/// which is the defect this file exists to catch.
const CLASSES_WITHOUT_A_SENTENCE_TODAY: usize = 38;

/// How many places build a failure whose class this scan cannot read, because it
/// is a variable rather than a literal.
///
/// **THE BLIND SPOT IS DECLARED, NOT HIDDEN.** A number here says the check
/// covers everything else; no number at all would let the blind spot grow while
/// the test stayed green, which is the shape of every silent check in the fault
/// ledger.
///
/// **THE ONE LEFT CANNOT BE CLOSED, AND THAT IS WHY IT IS ONE AND NOT ZERO.**
/// `mcp.rs` takes the class from the tool server's own status string: it is a
/// word from outside this repository, so no catalogue can be complete for it and
/// the window falling back to the raw name is the correct behaviour, not a gap.
/// The other one was closed by making `subflow.rs` write its five classes out
/// where the error is built instead of assembling the name from a status.
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
    let root = root();
    let mut found = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates")) else {
        panic!("the crates directory is where this test looks and it is not there");
    };
    for entry in crates.flatten() {
        walk(&entry.path().join("src"), &mut found);
    }
    found
}

/// What each place that builds a failure names it: the class when it is written
/// out, `None` when it is a variable this scan cannot follow.
fn classes_reported(text: &str) -> Vec<Option<String>> {
    // The unit-test module at the foot of a file invents classes to exercise the
    // code around them. They are not classes the engine reports.
    let production = text.split("#[cfg(test)]").next().unwrap_or_default();
    let mut found = Vec::new();
    for (at, _) in production.match_indices("ActionError::new(") {
        let after = production[at + "ActionError::new(".len()..].trim_start();
        match after.strip_prefix('"').and_then(|rest| {
            rest.find('"').map(|end| rest[..end].to_owned())
        }) {
            Some(class) => found.push(Some(class)),
            None => found.push(None),
        }
    }
    found
}

fn measured() -> (Vec<String>, usize) {
    let mut classes = Vec::new();
    let mut unreadable = 0;
    for file in every_source() {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for reported in classes_reported(&text) {
            match reported {
                Some(class) => classes.push(class),
                None => unreadable += 1,
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
    for known in ["engine_exit_error", "answer_not_json", "invalid_input"] {
        assert!(
            classes.iter().any(|class| class == known),
            "the scan did not find «{known}», which is written out in the source: \
             every number this file reports is worthless until it does"
        );
    }
}

/// **A COVERAGE MEASURE IS CHECKED AGAINST A LIST THAT IS NOT ITS OWN.**
///
/// A count of what the walker found says nothing: a walker that quietly stopped
/// opening a whole kind of file still returns hundreds, and «hundreds» reads as
/// «it looked everywhere». That is how four files went unread under a check
/// whose own guard was green, on this tree, on the same day this was written.
///
/// So the list comes from git, which does not share the walker's idea of what a
/// file is, and the demand is not a number but a name: every source file git
/// tracks under a crate must be one the walker opened.
#[test]
fn every_source_file_git_tracks_is_one_the_scan_opened() {
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

#[test]
fn every_failure_the_engine_reports_has_a_sentence() {
    let (classes, _) = measured();
    let mute: Vec<&String> = classes
        .iter()
        .filter(|class| {
            catalogue::look("en", &format!("{FAILURE_PREFIX}{class}"), &[]).is_none()
        })
        .collect();

    assert!(
        mute.len() <= CLASSES_WITHOUT_A_SENTENCE_TODAY,
        "{} classes reach the window with no sentence, and the seed says {}. \
         Whoever hit one of these read the bare class name.\n{mute:#?}",
        mute.len(),
        CLASSES_WITHOUT_A_SENTENCE_TODAY
    );
    assert!(
        CLASSES_WITHOUT_A_SENTENCE_TODAY.saturating_sub(mute.len()) <= HOW_STALE_A_SEED_MAY_BE,
        "the seed says {} and the tree holds {}: lower it, or the ceiling stays \
         up for whoever adds the next class",
        CLASSES_WITHOUT_A_SENTENCE_TODAY,
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
        CLASSES_THE_SCAN_CANNOT_READ_TODAY.saturating_sub(unreadable) <= HOW_STALE_A_SEED_MAY_BE,
        "the seed says {CLASSES_THE_SCAN_CANNOT_READ_TODAY} and the tree holds \
         {unreadable}: lower it"
    );
}
