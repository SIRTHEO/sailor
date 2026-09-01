//! Working bases are **declared**, not compiled in — and a base that could not
//! be read is not confused with an empty one. THE QUESTION THESE TESTS DEFEND:
//! does "zero repos" mean there are none, or that I could not look? It meant
//! both, with no way to tell — `repos_under` met an unreadable directory, did
//! `continue`, and returned a shorter list with exit 0. Fault 12's shape:
//! *empty* standing in for *I do not know*.

use inventory::{default_roots_from, repos_under};
use std::fs;
use std::path::PathBuf;

/// A throwaway directory, deleted and rebuilt every run.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bases-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A repo is recognised by the `.claude/` it carries, as elsewhere in the crate.
fn repo(base: &PathBuf, name: &str) {
    fs::create_dir_all(base.join(name).join(".claude")).unwrap();
}

#[test]
fn a_base_that_cannot_be_read_is_reported_not_swallowed() {
    let missing = temp("unreadable").join("this-does-not-exist");
    let survey = repos_under(&[missing.clone()]);

    assert!(
        survey.roots.is_empty(),
        "a base that does not exist carries no repos"
    );
    assert_eq!(
        survey
            .unreadable
            .iter()
            .map(|u| &u.path)
            .collect::<Vec<_>>(),
        vec![&missing],
        "the unreadable base must show up in the survey rather than vanish: \
         it is the difference between \"there are none\" and \"I could not look\""
    );
    assert!(
        !survey.unreadable[0].reason.is_empty(),
        "the reader has to know why, not only that"
    );
}

#[test]
fn an_empty_base_is_not_the_same_as_an_unreadable_one() {
    let base = temp("empty");
    let survey = repos_under(&[base]);

    assert!(
        survey.roots.is_empty(),
        "an empty directory carries no repos"
    );
    assert!(
        survey.unreadable.is_empty(),
        "an empty directory was read perfectly well: calling it unreadable \
         would be the opposite error, and just as serious"
    );
}

#[test]
fn the_bases_that_are_declared_are_the_ones_searched() {
    let base = temp("declared");
    repo(&base, "first");
    repo(&base, "second");

    let survey = repos_under(&[base]);
    let mut names: Vec<&str> = survey.roots.iter().map(|r| r.label.as_str()).collect();
    names.sort_unstable();

    assert_eq!(names, vec!["first", "second"]);
    assert!(survey.unreadable.is_empty());
}

/// THE FIRST OF TWO CURES, AND ALONE IT IS NOT ENOUGH: taking one person's
/// directories out without being able to say "no bases declared" switches the
/// inventory off in silence.
#[test]
fn with_nothing_declared_the_survey_says_so_instead_of_saying_zero() {
    let home = temp("home-with-no-declaration");
    let survey = default_roots_from(&home, &[]);

    assert!(
        !survey.bases_declared,
        "with nothing declared the inventory has to be able to say so: a \
         \"zero repos\" born of never having looked is a false answer"
    );
    assert_eq!(
        survey.roots.len(),
        1,
        "the home remains: it is not a working base, but it is always there"
    );
    assert!(survey.roots[0].is_home);
}

/// THE SECOND CURE, AND ALONE IT IS NOT ENOUGH EITHER: being able to say "I
/// could not look" while the directories stay compiled in leaves the defect
/// exactly where it is. It also blocks the return, because the violation was
/// not a fault of logic but of habit, and habits come back in by the door.
/// It reads the source off disk because there is no way to ask the compiler
/// "name nobody's directory".
#[test]
fn no_ones_personal_folders_are_compiled_into_the_binary() {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("lib.rs"),
    )
    .expect("the crate source");

    let body: String = source
        .lines()
        .skip_while(|line| !line.starts_with("pub fn declared_bases"))
        .take_while(|line| !line.starts_with('}'))
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        body.contains("SAILOR_WORK_ROOTS"),
        "the body of `declared_bases` was not found: this test is watching nothing"
    );
    // **IT WATCHES THE SHAPE, NOT A LIST OF NAMES.** A list is got around by
    // picking a fourth directory, and to exist it has to publish the very names
    // it keeps out. What must not be here is the home: `declared_bases` reads a
    // declaration, and derives nothing from `$HOME`.
    for from_the_home in ["home", "HOME"] {
        assert!(
            !body.contains(from_the_home),
            "`declared_bases` derives a base from \"{from_the_home}\": one person's \
             directories are not a fact about the machine. They are declared with \
             `SAILOR_WORK_ROOTS`, or with the `work-roots` file in Sailor's config."
        );
    }
}
