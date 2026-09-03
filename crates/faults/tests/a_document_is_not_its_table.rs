//! Writing the register out replaces the rows and nothing else.
//!
//! **THE PROSE AROUND THE TABLE IS THE DOCUMENT.** Writing the rendered rows
//! over the file dropped a preamble saying what the register is for and a
//! closing section reading it whole — 164 lines — and the command reported
//! success. That happened on 03/09, once, before this test existed.

use faults::{render_into, Fault};

fn a_fault(number: i64, what: &str) -> Fault {
    Fault {
        number,
        happened_on: "03/09".to_owned(),
        what_happened: what.to_owned(),
        how_it_showed: "reading it".to_owned(),
        what_would_prevent: "a check".to_owned(),
        status: "**aperto**".to_owned(),
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
