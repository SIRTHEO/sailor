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

/// A cell the table cannot hold must be refused, not written and lost.
///
/// **BORN RED, WITH A ROW ALREADY GONE.** Fault 60 in the store carried newlines
/// inside a cell: rendered, that row breaks into pieces with the wrong number of
/// columns and `parse` drops every one. The round trip above could not see it —
/// it fills the store from the table, which holds only what a table can hold.
#[test]
fn a_cell_the_table_cannot_hold_is_refused_at_the_door() {
    let store = Faults::open(scratch("newline")).expect("opening");
    let refused = store.record(&Draft {
        happened_on: "01/09".to_owned(),
        what_happened: "a fault whose story\nruns over two lines".to_owned(),
        how_it_showed: "by rendering it".to_owned(),
        what_would_prevent: "this test".to_owned(),
        status: "**aperto**".to_owned(),
    });

    let Err(said) = refused else {
        panic!(
            "the store took a cell with a newline in it. Rendered, that row \
             breaks into pieces with the wrong number of columns, and parse \
             drops every one of them: the fault disappears from the register \
             and nothing fails"
        );
    };
    let said = said.to_string();
    assert!(
        said.contains("newline"),
        "whoever writes must be told which character cannot cross: {said}"
    );
}

/// **«NOT OPEN» AND «NOT UNDERSTOOD» MUST NOT BE THE SAME ANSWER.**
///
/// The old predicate answered yes or no, so a status it could not read left the
/// open tally silently, and the total moved the reassuring way. Latent when
/// found — every row on record classified — which makes it no smaller: a latent
/// fault is one nobody has met yet.
#[test]
fn a_status_nobody_taught_this_is_refused_and_never_counted_as_closed() {
    assert_eq!(
        faults::standing_of("closed on the first, with a mutant"),
        faults::Standing::Unrecognised,
        "a status in another language must read as unrecognised, never as closed"
    );
    assert_eq!(
        faults::standing_of("**riaperto** il 02/09"),
        faults::Standing::Unrecognised,
        "a nuance nobody taught this must be refused, not silently closed"
    );

    let store = Faults::open(scratch("stato-ignoto")).expect("opening");
    let refused = store.record(&Draft {
        happened_on: "01/09".to_owned(),
        what_happened: "something".to_owned(),
        how_it_showed: "by running it".to_owned(),
        what_would_prevent: "this test".to_owned(),
        status: "half done, half not".to_owned(),
    });
    assert!(
        refused.is_err(),
        "a status the count cannot read went into the store, and the fault it \
         describes has already left the open tally without anything failing"
    );

    // And the half-closed reading is asked before the closed one, because the
    // second is a prefix of nothing and the first begins with the other's word.
    assert_eq!(
        faults::standing_of("**chiuso in parte** il 01/09"),
        faults::Standing::PartlyClosed,
        "asking in the other order takes every half-closed row out of the tally"
    );
}

/// Every door into the store, not the two that were easy to find.
///
/// There are three ways a cell gets written — `record`, `restore`, `set_status`
/// — and a guard on two of them reads exactly like a guard. `set_status` is the
/// one nobody thinks of, because it looks like a state change rather than a
/// write of prose, and status *is* prose here.
#[test]
fn no_door_into_the_store_takes_a_cell_the_table_cannot_hold() {
    let store = Faults::open(scratch("doors")).expect("opening");
    let sound = Draft {
        happened_on: "01/09".to_owned(),
        what_happened: "something on one line".to_owned(),
        how_it_showed: "by running it".to_owned(),
        what_would_prevent: "this test".to_owned(),
        status: "**aperto**".to_owned(),
    };
    let written = store.record(&sound).expect("a sound row goes in");

    let broken = "closed\nover two lines";
    assert!(
        store.set_status(written.number, broken).is_err(),
        "«set_status» is a door too: status is prose, and prose with a newline \
         in it takes the whole row out of the register"
    );
    assert!(
        store
            .record(&Draft {
                status: broken.to_owned(),
                ..sound.clone()
            })
            .is_err(),
        "«record» let a broken status through"
    );
    assert!(
        store
            .restore(&Fault {
                number: 99,
                status: broken.to_owned(),
                happened_on: sound.happened_on.clone(),
                what_happened: sound.what_happened.clone(),
                how_it_showed: sound.how_it_showed.clone(),
                what_would_prevent: sound.what_would_prevent.clone(),
            })
            .is_err(),
        "«restore» let a broken status through"
    );

    // The separator is the other way a row comes apart, and it is the one a
    // person writes by accident: a cell holding « | » renders a row with seven
    // columns, which parse drops exactly like the broken one.
    assert!(
        store
            .record(&Draft {
                what_happened: "the flag reads on | off".to_owned(),
                ..sound.clone()
            })
            .is_err(),
        "a cell holding the column separator adds a column, and the row is \
         dropped on the way back just the same"
    );

    assert_eq!(
        store.all().expect("reading back").len(),
        1,
        "nothing that was refused may have landed anyway"
    );
}

/// And the refusal is not cosmetic: this is what it prevents.
///
/// Kept separate from the check above so that removing the guard shows the
/// consequence, not just a missing error. Rendering a row with a newline in it
/// and reading it back loses the row entirely.
#[test]
fn a_newline_in_a_cell_makes_the_row_vanish_on_the_way_back() {
    let broken = Fault {
        number: 60,
        happened_on: "01/09".to_owned(),
        what_happened: "a story\nover two lines".to_owned(),
        how_it_showed: "by rendering it".to_owned(),
        what_would_prevent: "refusing it at the door".to_owned(),
        status: "**aperto**".to_owned(),
    };

    let back = faults::parse(&faults::render(&[broken]));

    assert!(
        back.is_empty(),
        "this test records why the door is shut. If the row now survives the \
         round trip, the rendering learned to escape newlines, and the guard \
         in the store can be reconsidered - deliberately, not by accident"
    );
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
