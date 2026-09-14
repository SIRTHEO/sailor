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

/// A full six-column register, kept here rather than read from the repository:
/// the published page carries only the open faults, and the round trip must
/// still meet every shape a cell takes — an escaped bar, bold, both date
/// forms, and all three standings.
const A_REGISTER: &str = "\
# The faults

| # | date | what happened | how it showed | what would have prevented it | status |
|---|---|---|---|---|---|
| 1 | 29/08 | **A step reported success on a non-zero exit.** The run went on | reading the ledger by hand | a non-zero exit breaks the step | **open** |
| 2 | 2026-08-30 | a flag read `on\\|off` and split the row | the page showed eight columns | escaping the bar on the way out | **closed** on 31/08 — with a mutant |
| 3 | 31/08 | a count «reassured» instead of measuring | counting the rows by hand | a judge that counts | **closed in part** on 01/09, the measure still to make |
| 4 | 01/09 | a port held by an orphan process blocked the start | the window would not open | the store names every process it starts | **open** — the sweep exists, the start is not guarded |
| 5 | 2026-09-02 | two sessions took the same number | two rows 27 in one table | the store hands out the number | **closed** on 03/09 |

What the table says is prose, and the round trip does not read it.
";

const ROWS_IN_THE_REGISTER: usize = 5;

/// The register the checks below walk, with the receipt of what they walked.
fn table() -> String {
    workspace::measured_against(
        faults_in(A_REGISTER),
        "faults read from the fixture register and carried across",
        A_REGISTER.lines().count(),
        "lines the fixture holds",
    );
    A_REGISTER.to_owned()
}

/// What the judge actually walks: the file holds twice as many lines.
fn faults_in(text: &str) -> usize {
    faults::parse(text).len()
}

/// Every row of one table that does not come back out of one store the way it
/// went in. Empty is the migration losing nothing.
///
/// Compared by number, not by position: the file's order is a reader's choice,
/// not data, so pairing rows positionally would call an identical row "changed"
/// for sitting two lines lower.
fn what_the_crossing_changes(source: &str, store: &Faults) -> Vec<String> {
    let read = faults::parse(source);
    let mut lost = Vec::new();
    for fault in &read {
        if store.restore(fault).is_err() {
            lost.push(format!("fault {} would not go back in with its number", fault.number));
        }
    }

    let back = store.all().expect("reading back");
    if back.len() != read.len() {
        lost.push(format!("{} rows went in and {} came out", read.len(), back.len()));
    }

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
        match by_number.get(&number) {
            None => lost.push(format!("fault {number} did not come out of the store")),
            Some(now) if *now != line => {
                lost.push(format!("fault {number} changed while crossing the store"))
            }
            Some(_) => {}
        }
        seen += 1;
    }
    if seen != read.len() {
        lost.push(format!("{seen} rows were compared and the table holds {}", read.len()));
    }
    lost
}

/// Every row that says nothing about what would have stopped it: that column
/// is the line between this register and a diary.
fn rows_that_are_a_diary(read: &[Fault]) -> Vec<i64> {
    read.iter()
        .filter(|fault| {
            fault.what_would_prevent.is_empty()
                || fault.what_happened.is_empty()
                || fault.how_it_showed.is_empty()
                || fault.status.is_empty()
        })
        .map(|fault| fault.number)
        .collect()
}

/// What the numbers coming in are, once sorted: `1..=N` or something else.
fn the_numbers_of(read: &[Fault]) -> Vec<i64> {
    let mut numbers: Vec<i64> = read.iter().map(|fault| fault.number).collect();
    numbers.sort_unstable();
    numbers
}

/// Nothing is lost coming in. The real rows, brought in and written back, must
/// come out identical — not equivalent: identical, because a cell's nuance is
/// the information.
#[test]
fn every_row_survives_the_move_word_for_word() {
    let source = table();
    let read = faults::parse(&source);
    assert_eq!(
        read.len(),
        ROWS_IN_THE_REGISTER,
        "the fixture register was not read whole"
    );

    let store = Faults::open(scratch("round-trip")).expect("opening");
    let lost = what_the_crossing_changes(&source, &store);
    assert!(lost.is_empty(), "{}", lost.join("; "));
}

/// A cell the table cannot hold must be refused, not written and lost.
///
/// The round trip above cannot see this: it fills the store from the table,
/// which holds only what a table can hold. See fault 60.
#[test]
fn a_cell_the_table_cannot_hold_is_refused_at_the_door() {
    let store = Faults::open(scratch("newline")).expect("opening");
    let refused = store.record(&Draft {
        happened_on: "01/09".to_owned(),
        what_happened: "a fault whose story\nruns over two lines".to_owned(),
        how_it_showed: "by rendering it".to_owned(),
        what_would_prevent: "this test".to_owned(),
        status: "**open**".to_owned(),
        standing: None,
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

/// **«NOT OPEN» AND «NOT UNDERSTOOD» MUST NOT BE THE SAME ANSWER.** A predicate
/// answering yes or no leaves the open tally silently, and the total moves the
/// reassuring way.
#[test]
fn a_status_nobody_taught_this_is_refused_and_never_counted_as_closed() {
    assert_eq!(
        faults::standing_of("closed on the first, with a mutant"),
        faults::Standing::Unknown,
        "a status that never wrote the marker must read as unrecognised, never as closed"
    );
    assert_eq!(
        faults::standing_of("**chiuso** il 02/09"),
        faults::Standing::Unknown,
        "a marker in the language the register left behind must be refused, not silently closed"
    );
    assert_eq!(
        faults::standing_of("**reopened** on 02/09"),
        faults::Standing::Unknown,
        "a nuance nobody taught this must be refused, not silently closed"
    );

    let store = Faults::open(scratch("unknown-status")).expect("opening");
    let refused = store.record(&Draft {
        happened_on: "01/09".to_owned(),
        what_happened: "something".to_owned(),
        how_it_showed: "by running it".to_owned(),
        what_would_prevent: "this test".to_owned(),
        status: "half done, half not".to_owned(),
        standing: None,
    });
    assert!(
        refused.is_err(),
        "a status the count cannot read went into the store, and the fault it \
         describes has already left the open tally without anything failing"
    );

    // And the half-closed reading is asked before the closed one, because the
    // second is a prefix of nothing and the first begins with the other's word.
    assert_eq!(
        faults::standing_of("**closed in part** on 01/09"),
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
        status: "**open**".to_owned(),
        standing: None,
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
                standing: faults::Standing::Unknown,
                happened_on: sound.happened_on.clone(),
                happened: faults::Happening::read(&sound.happened_on),
                what_happened: sound.what_happened.clone(),
                how_it_showed: sound.how_it_showed.clone(),
                what_would_prevent: sound.what_would_prevent.clone(),
                public_summary: None,
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
        happened: faults::Happening::DayAndMonth { month: 9, day: 1 },
        what_happened: "a story\nover two lines".to_owned(),
        how_it_showed: "by rendering it".to_owned(),
        what_would_prevent: "refusing it at the door".to_owned(),
        status: "**open**".to_owned(),
        standing: faults::Standing::Open,
        public_summary: None,
    };

    let back = faults::parse(&faults::render(&[broken]));

    assert!(
        back.is_empty(),
        "this test records why the door is shut. If the row now survives the \
         round trip, the rendering learned to escape newlines, and the guard \
         in the store can be reconsidered - deliberately, not by accident"
    );
}

/// **THE SUMMARY HAS A DOOR OF ITS OWN**, and the doors that rewrite a row do
/// not pass through it: restoring the same number keeps what users read.
#[test]
fn a_public_summary_is_written_by_its_own_door_and_survives_a_restore() {
    let store = Faults::open(scratch("summary")).expect("opening");
    let written = store
        .record(&Draft {
            happened_on: "01/09".to_owned(),
            what_happened: "a workshop story".to_owned(),
            how_it_showed: "by running it".to_owned(),
            what_would_prevent: "this test".to_owned(),
            status: "**open**".to_owned(),
            standing: None,
        })
        .expect("recording");
    assert_eq!(written.public_summary, None, "a fault is born with no summary");

    assert!(store.set_public_summary(written.number, "  ").is_err(), "an empty summary was written");
    assert!(
        store.set_public_summary(written.number, "one line\nand another").is_err(),
        "a summary that breaks a row was written"
    );
    assert!(
        store.set_public_summary(99, "A summary.").is_err(),
        "a summary for a fault that is not there was written"
    );

    store
        .set_public_summary(written.number, "The window forgets a flow you renamed.")
        .expect("a summary");
    let row = faults::parse(&faults::render(&store.all().expect("reading back"))).remove(0);
    store.restore(&row).expect("restoring the row");

    assert_eq!(
        store.get(written.number).expect("the fault").public_summary.as_deref(),
        Some("The window forgets a flow you renamed."),
        "restoring the row erased what users read"
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
        status: "**open**".to_owned(),
        standing: None,
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
        "**open**",
        "**open** — the procedural defences are in force, the code is not",
        "**closed in part** on 01/09, reopened on 02/09",
        "**closed** on 01/09 — with a mutant",
    ] {
        store
            .record(&Draft {
                happened_on: "01/09".to_owned(),
                what_happened: "something".to_owned(),
                how_it_showed: "by running it".to_owned(),
                what_would_prevent: "a test".to_owned(),
                status: status.to_owned(),
                standing: None,
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
        .set_status(99, "**closed** today")
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
    let source = table();
    let unfinished = rows_that_are_a_diary(&faults::parse(&source));
    assert!(
        unfinished.is_empty(),
        "these faults do not say what would have prevented them, or leave another \
         column empty: {unfinished:?}"
    );
}

/// No twin numbers and no gaps among those coming in: the migration keeps them,
/// and from here on they cannot be got wrong.
#[test]
fn the_numbers_that_come_in_have_no_gaps_and_no_twins() {
    let source = table();
    let numbers = the_numbers_of(&faults::parse(&source));
    assert_eq!(
        numbers,
        (1..=numbers.len() as i64).collect::<Vec<i64>>(),
        "the numbers coming in are not 1..N without gaps: the migration would \
         carry a defect in instead of leaving it out"
    );
}

/// A table of its own, holding the rows it is handed.
fn a_table(rows: &str) -> String {
    format!(
        "# The faults\n\n| # | when | what happened | how it showed | what would prevent it | status |\n| --- | --- | --- | --- | --- | --- |\n{rows}"
    )
}

/// One row of the table, written the way the register writes it.
fn a_row(number: i64, prevention: &str) -> String {
    format!("| {number} | 01/09 | something went wrong | by running it | {prevention} | **open** |\n")
}

/// **A SOUND TABLE IS NOT A WORKING CHECK.** The three verdicts above are
/// asked of a register with nothing to refuse, so here a defect is planted in
/// a table of its own and the same verdicts must name the row.
#[test]
fn a_defect_planted_in_a_throwaway_table_is_found_by_every_verdict() {
    let source = a_table(&format!("{}{}", a_row(1, "this test"), a_row(2, "this test")));
    let store = Faults::open(scratch("planted-sound")).expect("opening");
    assert!(
        what_the_crossing_changes(&source, &store).is_empty(),
        "the control: a sound table must cross the store unchanged"
    );
    assert!(rows_that_are_a_diary(&faults::parse(&source)).is_empty());
    assert_eq!(the_numbers_of(&faults::parse(&source)), vec![1, 2]);

    // A cell padded with spaces is read trimmed and written back trimmed: the
    // row that comes out is not the row that went in, word for word.
    let source = a_table(&format!(
        "{}| 2 | 01/09 | something went wrong | by running it |  this test  | **open** |\n",
        a_row(1, "this test")
    ));
    let store = Faults::open(scratch("planted-padded")).expect("opening");
    let changed = what_the_crossing_changes(&source, &store);
    assert!(
        changed.iter().any(|said| said.contains("fault 2 changed while crossing the store")),
        "the row that does not survive the crossing was not named: {changed:?}"
    );

    // A row that says nothing about what would have stopped it is a diary
    // entry, and the column being empty is how it looks.
    let source = a_table(&format!("{}{}", a_row(1, "this test"), a_row(2, "")));
    assert_eq!(
        rows_that_are_a_diary(&faults::parse(&source)),
        vec![2],
        "the row with no prevention in it was not named"
    );

    // And a gap in the numbers is a row the migration would carry in wrong.
    let source = a_table(&format!("{}{}", a_row(1, "this test"), a_row(3, "this test")));
    let numbers = the_numbers_of(&faults::parse(&source));
    assert_eq!(numbers, vec![1, 3]);
    assert_ne!(
        numbers,
        (1..=numbers.len() as i64).collect::<Vec<i64>>(),
        "a gap in the numbers read as 1..N"
    );
}

/// **THE RECEIPT COUNTS WHAT WAS WALKED**, not the lines around it.
#[test]
fn the_receipt_counts_the_faults_and_not_the_lines_of_the_file() {
    let source = a_table(&format!("{}{}{}", a_row(1, "a"), a_row(2, "b"), a_row(3, "c")));

    assert_eq!(faults_in(&source), 3, "three rows went in");
    assert!(
        source.lines().count() > 3,
        "the file holds heading and rule as well, which is why the two numbers differ"
    );
}
