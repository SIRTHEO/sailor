//! The projects Sailor knows, and the one thing that list has to get right.
//!
//! **A ROOT IS FOUND BY WALKING UP, SO SAILOR KNEW ONLY THE ONE IT STOOD IN.**
//! `find_root` answers "where am I", nothing answered "which projects are
//! there", so a window shows the current directory and switching means
//! quitting. The register is a file in the home, not history.

use flow::workspace::{known_in, remember_in, standing_of, Standing, MARKER};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A scratch directory with a counter in the name: two tests in the same second
/// would otherwise share it, which is fault 21.
fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sailor-known-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// A project that has declared itself, with the marker in place.
fn project(under: &Path, name: &str) -> PathBuf {
    let root = under.join(name);
    fs::create_dir_all(&root).expect("project directory");
    fs::write(root.join(MARKER), "{}\n").expect("the marker");
    root
}

/// **THE CONTROL, AND IT COMES FIRST.** An empty home has to read as an empty
/// list, not as an error: a register that fails when nobody has opened anything
/// would make every later assertion pass for the wrong reason.
#[test]
fn a_home_that_has_seen_nothing_answers_with_an_empty_list() {
    let home = scratch("empty-home");
    let seen = known_in(&home).expect("an empty home is not a failure");
    assert!(
        seen.is_empty(),
        "a home nobody has used lists {} projects",
        seen.len()
    );
}

/// What the register is for: two projects opened, both listed, in the order
/// they were last seen — the most recent first, because that is the one being
/// looked for.
#[test]
fn the_projects_you_opened_come_back_most_recent_first() {
    let home = scratch("two");
    let first = project(&home, "alfa");
    let second = project(&home, "beta");

    remember_in(&home, &first, 1_000).expect("the first is remembered");
    remember_in(&home, &second, 2_000).expect("and the second");

    let seen = known_in(&home).expect("the register reads back");
    let names: Vec<&str> = seen.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["beta", "alfa"], "the most recent has to lead");
}

/// **SEEING A PROJECT AGAIN MOVES `last_seen` AND LEAVES `first_seen` ALONE.**
/// The two dates answer different questions — "since when do I work on this"
/// and "was I here today" — and a register that overwrote the first would lose
/// the only one that cannot be reconstructed.
#[test]
fn opening_a_project_again_keeps_the_day_it_was_first_seen() {
    let home = scratch("again");
    let root = project(&home, "alfa");

    remember_in(&home, &root, 1_000).expect("first time");
    remember_in(&home, &root, 5_000).expect("and again, later");

    let seen = known_in(&home).expect("the register reads back");
    assert_eq!(seen.len(), 1, "the same project was written twice");
    assert_eq!(seen[0].first_seen, 1_000, "the first sighting moved");
    assert_eq!(seen[0].last_seen, 5_000, "the last sighting did not move");
}

/// **A PROJECT WHOSE MARKER IS GONE STAYS ON THE LIST, AND SAYS SO.** Dropping
/// it silently is the worse of the two: whoever opened it yesterday and cannot
/// find it today learns nothing, and a list that quietly shrinks is the shape
/// of fault 12 — a reading that cannot tell "none" from "I could not look".
#[test]
fn a_project_that_lost_its_marker_is_still_listed_and_reads_as_gone() {
    let home = scratch("gone");
    let there = project(&home, "alfa");
    let removed = project(&home, "beta");
    remember_in(&home, &there, 1_000).expect("the one that stays");
    remember_in(&home, &removed, 2_000).expect("the one that goes");

    fs::remove_file(removed.join(MARKER)).expect("the marker is taken away");

    let seen = known_in(&home).expect("the register reads back");
    assert_eq!(
        seen.len(),
        2,
        "the one that lost its marker fell off the list"
    );

    let gone = seen
        .iter()
        .find(|e| e.name == "beta")
        .expect("beta is listed");
    assert!(
        matches!(standing_of(gone), Standing::Gone),
        "beta reads as still declared"
    );

    // The other direction, and it is what makes the line above worth anything:
    // a check that answered `Gone` for everything would satisfy it too.
    let here = seen
        .iter()
        .find(|e| e.name == "alfa")
        .expect("alfa is listed");
    assert!(
        matches!(standing_of(here), Standing::Declared),
        "alfa reads as gone"
    );
}

/// A register written by a newer Sailor must not blank the list of an older
/// one. It is fault 8 in the place it would hurt most: the projects.
#[test]
fn a_register_carrying_unknown_fields_still_reads() {
    let home = scratch("unknown");
    let root = project(&home, "alfa");
    fs::write(
        home.join("workspaces.json"),
        format!(
            r#"{{"workspaces":[{{"root":{:?},"name":"alfa","first_seen":1,"last_seen":2,"colour":"blue"}}]}}"#,
            root.display().to_string()
        ),
    )
    .expect("a register from a newer version");

    let seen = known_in(&home).expect("an unknown field is not a reason to discard");
    assert_eq!(seen.len(), 1, "the entry was thrown away over a field");
    assert_eq!(seen[0].name, "alfa");
}
