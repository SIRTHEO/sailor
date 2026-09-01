//! The migration loses nothing, and whoever writes no longer picks the number.
//!
//! The test that counts is the round trip: read the table, put it in the store,
//! write it back, compare row by row. Nobody would re-check afterwards, because
//! the source would already be gone.

use faults::{Draft, Fault, Faults};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "faults-test-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir.join(faults::FAULTS_FILE)
}

fn table() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/faults")
        .join("docs/guasti-incontrati.md");
    std::fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("reading {}: {error}", file.display()))
}

/// Nothing is lost coming in. The real rows, brought in and written back, must
/// come out identical — not equivalent: identical, because a cell's nuance is
/// the information.
#[test]
fn every_row_survives_the_move_word_for_word() {
    let source = table();
    let read = faults::parse(&source);
    assert!(
        read.len() > 40,
        "the table emptied under the migration's feet: {} rows",
        read.len()
    );

    let store = Faults::open(scratch("round-trip")).expect("opening");
    for fault in &read {
        store
            .restore(fault)
            .expect("putting the row back with its number");
    }

    let back = store.all().expect("reading back");
    assert_eq!(
        back.len(),
        read.len(),
        "{} rows went in and {} came out",
        read.len(),
        back.len()
    );

    // Compared by number, not by position: the file's order is a reader's
    // choice, not data, so pairing rows positionally would call an identical
    // row "changed" for sitting two lines lower.
    let rewritten = faults::render(&back);
    let mut by_number: std::collections::BTreeMap<i64, &str> = std::collections::BTreeMap::new();
    for line in rewritten.lines() {
        let number: i64 = line
            .trim_matches('|')
            .split(" | ")
            .next()
            .and_then(|first| first.trim().parse().ok())
            .expect("every rendered row starts with its number");
        by_number.insert(number, line);
    }

    let mut seen = 0usize;
    for line in source.lines().map(str::trim) {
        let Some(number) = line
            .strip_prefix('|')
            .and_then(|rest| rest.split(" | ").next())
            .and_then(|first| first.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let now = by_number
            .get(&number)
            .unwrap_or_else(|| panic!("fault {number} did not come out of the store"));
        assert_eq!(
            &line, now,
            "fault {number} changed while crossing the store"
        );
        seen += 1;
    }
    assert_eq!(seen, read.len(), "not every row was compared");
}

/// The store hands out the number, and two calls take two.
///
/// While whoever wrote picked it by looking at the last row of a file, two
/// branches that cannot see each other took the same one. No test could stop
/// that, because a test looks at one branch at a time.
#[test]
fn the_store_hands_out_the_number_and_never_the_same_one_twice() {
    let store = Faults::open(scratch("numbers")).expect("opening");
    let draft = |what: &str| Draft {
        happened_on: "01/09".to_owned(),
        what_happened: what.to_owned(),
        how_it_showed: "by running it".to_owned(),
        what_would_prevent: "a test that is born red".to_owned(),
        status: "**aperto**".to_owned(),
    };

    let first = store.record(&draft("the first")).expect("recording");
    let second = store.record(&draft("the second")).expect("recording");

    assert_eq!(first.number, 1);
    assert_eq!(
        second.number, 2,
        "the second does not get the first's number"
    );
    assert_ne!(
        first.number, second.number,
        "two different faults cannot carry the same number: it is why the \
         number left the hands of whoever writes"
    );
}

/// The open count is computed, and half-closed counts as open. Copied by hand
/// it was wrong in four documents out of four; here there is no second place
/// to copy it to.
#[test]
fn a_half_closed_fault_still_counts_as_open() {
    let store = Faults::open(scratch("count")).expect("opening");
    for status in [
        "**aperto**",
        "**aperto** — le difese di procedura sono in vigore, il codice no",
        "**chiuso in parte** il 01/09, riaperto il 02/09",
        "**chiuso** il 01/09 — con mutante",
    ] {
        store
            .record(&Draft {
                happened_on: "01/09".to_owned(),
                what_happened: "something".to_owned(),
                how_it_showed: "by running it".to_owned(),
                what_would_prevent: "a test".to_owned(),
                status: status.to_owned(),
            })
            .expect("recording");
    }

    assert_eq!(
        store.still_open().expect("counting"),
        3,
        "open with one more nuance is still open, and so is half-closed: a \
         middle state says which half is done, it does not take the row out \
         of the count"
    );
}

/// Changing status is the only thing that happens to a fault afterwards, and a
/// number that does not exist is an error with a name instead of a silence.
#[test]
fn closing_a_fault_that_does_not_exist_says_so() {
    let store = Faults::open(scratch("unknown")).expect("opening");
    let refused = store
        .set_status(99, "**chiuso** oggi")
        .expect_err("a fault that is not there cannot be closed");
    assert!(refused.to_string().contains("99"), "{refused}");
}

/// A store written by a newer binary is recognised by name instead of looking
/// broken. Reading it as damage once cost half a day.
#[test]
fn a_newer_store_says_it_is_newer_and_not_broken() {
    let path = scratch("newer");
    Faults::open(&path).expect("creating it");
    let connection = rusqlite::Connection::open(&path).expect("reopening it by hand");
    connection
        .pragma_update(None, "user_version", 99_i64)
        .expect("raising the version");
    drop(connection);

    let said = match Faults::open(&path) {
        Ok(_) => panic!("a newer store must not open"),
        Err(refused) => refused.to_string(),
    };
    assert!(said.contains("99"), "{said}");
    assert!(
        said.contains("not broken"),
        "whoever reads must see it is newer, not damaged: {said}"
    );
}

/// An entry without `what_would_prevent` is not finished: that column is the
/// line between this and a diary.
#[test]
fn a_row_without_the_check_that_would_have_stopped_it_is_not_finished() {
    let read = faults::parse(&table());
    for fault in &read {
        assert!(
            !fault.what_would_prevent.is_empty(),
            "fault {} does not say what would prevent it",
            fault.number
        );
        assert!(!fault.what_happened.is_empty(), "{} is empty", fault.number);
        assert!(
            !fault.how_it_showed.is_empty(),
            "{} does not say how it showed",
            fault.number
        );
        assert!(!fault.status.is_empty(), "{} has no status", fault.number);
    }
}

/// No twin numbers and no gaps among those coming in: the migration keeps them,
/// and from here on they cannot be got wrong.
#[test]
fn the_numbers_that_come_in_have_no_gaps_and_no_twins() {
    let read: Vec<Fault> = faults::parse(&table());
    let mut numbers: Vec<i64> = read.iter().map(|f| f.number).collect();
    numbers.sort_unstable();
    let mut expected: Vec<i64> = (1..=numbers.len() as i64).collect();
    expected.sort_unstable();
    assert_eq!(
        numbers, expected,
        "the numbers coming in are not 1..N without gaps: the migration would \
         carry a defect in instead of leaving it out"
    );
}
