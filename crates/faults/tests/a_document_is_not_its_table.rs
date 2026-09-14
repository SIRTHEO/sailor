//! Writing the register out replaces the rows and nothing else.
//!
//! Writing the rendered rows over the file dropped a preamble and a closing
//! section — 164 lines — and the command reported success. That happened on
//! 03/09, once, before this test existed.

use faults::{render_into, Fault};

fn a_fault(number: i64, what: &str) -> Fault {
    Fault {
        number,
        happened_on: "03/09".to_owned(),
        happened: faults::Happening::DayAndMonth { month: 9, day: 3 },
        what_happened: what.to_owned(),
        how_it_showed: "reading it".to_owned(),
        what_would_prevent: "a check".to_owned(),
        status: "**open**".to_owned(),
        standing: faults::Standing::Open,
    }
}

#[test]
fn what_is_not_a_row_is_left_where_it_was() {
    let document = "# The faults\n\nWhy this file is not a diary.\n\n\
         | # | date | what |\n|---|---|---|\n| 1 | 01/09 | the old text |\n\n\
         ## What the table says, read whole\n\nAlmost half were seen by running.\n";

    let written = render_into(document, &[a_fault(1, "the new text")]);

    assert!(
        written.contains("Why this file is not a diary."),
        "the preamble is gone: {written}"
    );
    assert!(
        written.contains("## What the table says, read whole"),
        "the closing section is gone: {written}"
    );
    assert!(
        written.contains("| # | date | what |"),
        "the heading is the document's, not the renderer's: {written}"
    );
    assert!(written.contains("the new text"), "{written}");
    assert!(
        !written.contains("the old text"),
        "the row was not replaced: {written}"
    );
}

/// The public page keeps its prose too, and carries only the open faults in
/// four columns: the headline, and the status up to its first sentence.
#[test]
fn the_public_page_carries_the_open_faults_in_four_columns_and_keeps_its_prose() {
    let document = "# Open faults\n\nWhat this page is.\n\n\
         | # | since | what goes wrong | status |\n|---|---|---|---|\n| 9 | 01/09 | stale | **open** |\n\n\
         **One fault is still open.**\n";
    let mut headline = a_fault(1, "**`sailor flow cost` lies by 4.3 times.** The rest of the story");
    headline.status = "**closed in part** on 01/09 — the lie is closed. The rest | later".to_owned();
    headline.standing = faults::Standing::PartlyClosed;
    let mut closed = a_fault(2, "a repaired fault");
    closed.status = "**closed** on 02/09".to_owned();
    closed.standing = faults::Standing::Closed;
    let mut sentence = a_fault(3, "A value in `lib.rs` reads a | b. Then more");
    sentence.status = "**open** — **nobody took it. yet** and more".to_owned();

    let written = faults::render_open_into(document, &[headline, closed, sentence]);

    assert!(written.contains("What this page is."), "the preamble is gone: {written}");
    assert!(written.contains("**One fault is still open.**"), "the closing sentence is gone: {written}");
    assert!(!written.contains("stale"), "the old row was not replaced: {written}");
    assert!(
        written.contains("| 1 | 03/09 | **`sailor flow cost` lies by 4.3 times.** | **closed in part** on 01/09 — the lie is closed. |"),
        "the headline row: {written}"
    );
    assert!(!written.contains("a repaired fault"), "a closed fault reached the page: {written}");
    assert!(
        written.contains("| 3 | 03/09 | A value in `lib.rs` reads a \\| b. | **open** — **nobody took it.** |"),
        "the first sentence, escaped, with its bold span closed: {written}"
    );
    let row = written.lines().find(|line| line.starts_with("| 1 |")).expect("row 1");
    let status = row.trim_end_matches(" |").rsplit(" | ").next().expect("a status cell");
    assert_eq!(
        faults::standing_of(status),
        faults::Standing::PartlyClosed,
        "the cut status must still be read by the one reading of the column"
    );
}

/// The store holding fewer rows than the document must not leave leftovers:
/// the run is replaced whole, not row by row.
#[test]
fn the_run_of_rows_is_replaced_whole() {
    let document = "|---|\n| 1 | a | b |\n| 2 | c | d |\n| 3 | e | f |\n\nafter\n";

    let written = render_into(document, &[a_fault(1, "only one")]);

    assert!(!written.contains("| 2 |"), "a row survived: {written}");
    assert!(!written.contains("| 3 |"), "a row survived: {written}");
    assert!(written.contains("after"), "{written}");
    assert!(
        written.contains("|---|"),
        "the separator is the document's: {written}"
    );
}
