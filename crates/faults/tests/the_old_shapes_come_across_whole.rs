//! A store written before the vocabulary existed crosses into it losing nothing.
//!
//! The rows are invented: the real register is prose people wrote, and this
//! test must be readable by anyone, on any machine.

use faults::{Faults, Happening, Standing};
use rusqlite::{params, Connection};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "faults-old-shapes-{label}-{}-{}",
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

/// The two date shapes and the four standings the free-text column held, plus
/// the row neither of them can read.
const AS_IT_WAS: [(i64, &str, &str); 5] = [
    (1, "28/08", "**closed** — the line was fixed and measured"),
    (
        2,
        "2026-09-04",
        "**open** — the run stopped before it reached the end",
    ),
    (
        3,
        "31/08",
        "**closed in part** on 01/09, reopened on 02/09: half the cure is in",
    ),
    (4, "the first of September", "shut, I think"),
    (5, "2026-09-06", "**closed**"),
];

/// A store in the shape the previous version wrote: six columns, no vocabulary.
fn a_store_of_the_old_shape(label: &str) -> PathBuf {
    let path = scratch(label);
    let connection = Connection::open(&path).expect("creating the old store");
    connection
        .execute_batch(
            "CREATE TABLE faults (
                 number INTEGER PRIMARY KEY,
                 happened_on TEXT NOT NULL,
                 what_happened TEXT NOT NULL,
                 how_it_showed TEXT NOT NULL,
                 what_would_prevent TEXT NOT NULL,
                 status TEXT NOT NULL
             );",
        )
        .expect("the old table");
    for (number, happened_on, status) in AS_IT_WAS {
        connection
            .execute(
                "INSERT INTO faults
                     (number, happened_on, what_happened, how_it_showed,
                      what_would_prevent, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    number,
                    happened_on,
                    format!("the {number}th thing that broke"),
                    "by running it",
                    "a check that reads the column back",
                    status,
                ],
            )
            .expect("a row of the old shape");
    }
    connection
        .pragma_update(None, "user_version", 1_i64)
        .expect("the old version");
    drop(connection);
    path
}

/// The prose is the only record of what somebody meant: the migration reads it,
/// it does not rewrite it.
#[test]
fn the_prose_of_every_old_row_survives_the_migration_word_for_word() {
    let path = a_store_of_the_old_shape("prose");
    let store = Faults::open(&path).expect("migrating");
    let all = store.all().expect("reading back");

    assert_eq!(all.len(), AS_IT_WAS.len(), "a row was lost on the way");
    for (fault, (number, happened_on, status)) in all.iter().zip(AS_IT_WAS) {
        assert_eq!(fault.number, number);
        assert_eq!(
            fault.happened_on, happened_on,
            "the date somebody wrote was rewritten instead of read"
        );
        assert_eq!(
            fault.status, status,
            "the nuance after the marker is the information, and it was dropped"
        );
    }
    workspace::measured(all.len(), "rows brought across from the old shape");
}

/// Three standings that do not merge, and a fourth for the row nobody classified.
#[test]
fn the_standings_come_out_in_the_vocabulary_and_the_unreadable_one_is_not_closed() {
    let path = a_store_of_the_old_shape("standings");
    let store = Faults::open(&path).expect("migrating");
    let read: Vec<Standing> = store
        .all()
        .expect("reading back")
        .iter()
        .map(|fault| fault.standing)
        .collect();

    assert_eq!(
        read,
        vec![
            Standing::Closed,
            Standing::Open,
            Standing::PartlyClosed,
            Standing::Unknown,
            Standing::Closed,
        ],
        "«shut, I think» is neither open nor closed: read as closed it leaves \
         the open tally, and the error goes the way that reassures"
    );

    let tally = store.tally().expect("counting");
    assert_eq!(tally.get(&Standing::Unknown), Some(&1), "{tally:?}");
    assert_eq!(tally.get(&Standing::Closed), Some(&2), "{tally:?}");
    assert_eq!(
        tally.values().sum::<usize>(),
        AS_IT_WAS.len(),
        "every row belongs to exactly one word of the vocabulary: {tally:?}"
    );
    assert_eq!(store.still_open().expect("counting"), 2, "{tally:?}");
}

/// **THE YEAR IS NOT GUESSED.** `28/08` says a day and a month and nothing else;
/// inventing the year would make the register sortable by making it wrong.
#[test]
fn a_date_with_no_year_says_so_and_no_year_is_invented() {
    let path = a_store_of_the_old_shape("dates");
    let store = Faults::open(&path).expect("migrating");
    let all = store.all().expect("reading back");

    assert_eq!(
        all.iter().map(|f| f.happened).collect::<Vec<_>>(),
        vec![
            Happening::DayAndMonth { month: 8, day: 28 },
            Happening::On {
                year: 2026,
                month: 9,
                day: 4
            },
            Happening::DayAndMonth { month: 8, day: 31 },
            Happening::Unknown,
            Happening::On {
                year: 2026,
                month: 9,
                day: 6
            },
        ],
        "two shapes went in and one format must come out, with the missing \
         year and the unreadable date each said out loud"
    );

    assert_eq!(all[0].happened.value(), "--08-28");
    assert_eq!(all[0].happened.reading(), "day-and-month");
    assert_eq!(all[3].happened.reading(), "unknown");
    assert_eq!(
        all[1].happened.value(),
        "2026-09-04",
        "the date that already carried its year keeps it"
    );
}

/// The columns hold words, not sentences: that is what makes them countable.
#[test]
fn the_new_columns_hold_only_the_words_of_the_vocabulary() {
    let path = a_store_of_the_old_shape("columns");
    Faults::open(&path).expect("migrating");

    let connection = Connection::open(&path).expect("reading the store by hand");
    let mut statement = connection
        .prepare("SELECT standing, happened_on_reading, happened_on_value FROM faults")
        .expect("the vocabulary columns exist");
    let rows: Vec<(String, String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("reading")
        .map(|row| row.expect("a row"))
        .collect();

    for (standing, reading, value) in &rows {
        Standing::from_word(standing).unwrap_or_else(|why| panic!("{why}"));
        Happening::from_columns(reading, value).unwrap_or_else(|why| panic!("{why}"));
        assert!(
            !standing.contains(' ') && !reading.contains(' '),
            "a column holding a sentence is the free text this replaced: \
             «{standing}», «{reading}»"
        );
    }
    assert_eq!(rows.len(), AS_IT_WAS.len());
}

/// A word nobody taught the reading is refused, never read as the reassuring one.
#[test]
fn a_word_outside_the_vocabulary_is_refused_instead_of_read_as_closed() {
    let path = a_store_of_the_old_shape("stranger");
    Faults::open(&path).expect("migrating");

    let connection = Connection::open(&path).expect("reopening by hand");
    connection
        .execute("UPDATE faults SET standing = 'chiuso' WHERE number = 1", [])
        .expect("writing a word from a half-done translation");
    drop(connection);

    let store = Faults::open(&path).expect("opening");
    let refused = store
        .all()
        .expect_err("a standing outside the vocabulary must not be read");
    let said = refused.to_string();
    assert!(
        said.contains("chiuso") && said.contains("standing"),
        "the refusal must name the column and the word: {said}"
    );
}

/// A store nobody may write still opens, and its rows are read from the prose.
///
/// The reading verbs run from a sandbox that grants no writes, so a migration
/// cannot be the price of listing the register.
#[test]
fn an_unmigrated_store_opened_for_reading_still_classifies_from_the_prose() {
    let path = a_store_of_the_old_shape("read-only");
    let store = Faults::open_for_reading(&path).expect("opening a store of the old shape");
    let all = store.all().expect("reading back");

    assert_eq!(all.len(), AS_IT_WAS.len());
    assert_eq!(all[3].standing, Standing::Unknown);
    assert_eq!(all[0].happened, Happening::DayAndMonth { month: 8, day: 28 });
    assert_eq!(store.still_open().expect("counting"), 2);
}

/// The rows go back out the way they came in: the register is the table.
#[test]
fn the_rendered_table_is_the_prose_that_went_in() {
    let path = a_store_of_the_old_shape("render");
    let store = Faults::open(&path).expect("migrating");
    let rendered = faults::render(&store.all().expect("reading back"));

    for (number, happened_on, status) in AS_IT_WAS {
        let row = rendered
            .lines()
            .find(|line| line.starts_with(&format!("| {number} | ")))
            .unwrap_or_else(|| panic!("fault {number} did not come out of the store"));
        assert!(
            row.contains(&format!("| {happened_on} |")) && row.ends_with(&format!("| {status} |")),
            "the rendered row lost what the old column held: {row}"
        );
    }
}
