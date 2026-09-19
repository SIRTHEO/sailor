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
        public_summary: None,
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

/// **THE REGISTER'S PROSE IS WORKSHOP MATERIAL.** The public page shows only
/// open faults given a summary for users, their standing and nothing of the
/// status prose, and a count that tells the page and the store apart.
#[test]
fn the_public_page_carries_only_open_faults_with_a_summary_and_keeps_its_prose() {
    let document = "# Open faults\n\nWhat this page is.\n\n\
         | # | since | what goes wrong | status |\n|---|---|---|---|\n| 9 | 01/09 | stale | **open** |\n\n\
         **Nine open faults are described on this page; zero more are kept only in the fault store.**\n\n\
         After the count.\n";
    let mut described = a_fault(1, "a workshop story naming a branch and a session");
    described.public_summary =
        Some("The cost of a run with a handed step reads too low | by a factor.".to_owned());
    described.status = "**closed in part** on 01/09 — the lie is closed. The rest later".to_owned();
    described.standing = faults::Standing::PartlyClosed;
    let undescribed = a_fault(2, "an open fault nobody summarised");
    let mut closed = a_fault(3, "a repaired fault");
    closed.public_summary = Some("A repaired defect.".to_owned());
    closed.status = "**closed** on 02/09".to_owned();
    closed.standing = faults::Standing::Closed;
    let mut cut = a_fault(4, "another workshop story");
    cut.public_summary = Some("A flow name in `lib.rs` is read as 4.3 flows.".to_owned());
    cut.status = "**open** — **nobody took it. yet** and more".to_owned();

    let written = faults::render_open_into(document, &[described, undescribed, closed, cut]);

    assert!(written.contains("What this page is."), "the preamble is gone: {written}");
    assert!(written.contains("After the count."), "the prose after the count is gone: {written}");
    assert!(!written.contains("stale"), "the old row was not replaced: {written}");
    assert!(
        written.contains("| 1 | 03/09 | The cost of a run with a handed step reads too low \\| by a factor. | **closed in part** |"),
        "the summarised row, escaped, with the standing alone as its status: {written}"
    );
    assert!(!written.contains("the lie is closed"), "the register's status prose reached the page: {written}");
    assert!(!written.contains("workshop story"), "the register's prose reached the page: {written}");
    assert!(!written.contains("| 2 |"), "an open fault with no summary reached the page: {written}");
    assert!(!written.contains("A repaired defect."), "a closed fault reached the page: {written}");
    assert!(
        written.contains("| 4 | 03/09 | A flow name in `lib.rs` is read as 4.3 flows. | **open** |"),
        "the open row's status is its standing alone: {written}"
    );
    assert!(!written.contains("nobody took it"), "the register's status prose reached the page: {written}");
    assert!(
        written.contains("**Two open faults are described on this page; one more is kept only in the fault store.**"),
        "the count sentence was not rewritten: {written}"
    );
    assert!(!written.contains("Nine open faults"), "the old count sentence survived: {written}");
    let row = written.lines().find(|line| line.starts_with("| 1 |")).expect("row 1");
    let status = row.trim_end_matches(" |").rsplit(" | ").next().expect("a status cell");
    assert_eq!(
        faults::standing_of(status),
        faults::Standing::PartlyClosed,
        "the cut status must still be read by the one reading of the column"
    );
}

/// **AN EMPTY TABLE IS STILL A PLACE.** Rows rendered into a header with no
/// rows under it went after the whole document, below the count sentence.
#[test]
fn rows_rendered_into_an_empty_table_sit_under_its_header() {
    let document = "# Open faults\n\n| # | since | what goes wrong | status |\n|---|---|---|---|\n\n\
         **Zero open faults are described on this page; zero more are kept only in the fault store.**\n";
    let mut described = a_fault(1, "a workshop story");
    described.public_summary = Some("A summary for users.".to_owned());

    let written = faults::render_open_into(document, &[described]);

    let lines: Vec<&str> = written.lines().collect();
    let separator = lines
        .iter()
        .position(|line| *line == "|---|---|---|---|")
        .expect("the separator is kept");
    assert!(
        lines[separator + 1].starts_with("| 1 |"),
        "the row is not under the header: {written}"
    );
    assert_eq!(
        written.matches(faults::COUNT_SENTENCE_END).count(),
        1,
        "the count sentence was added a second time: {written}"
    );
}

#[test]
fn the_count_sentence_is_written_for_none_one_and_many() {
    assert_eq!(
        faults::count_sentence(0, 1),
        "**Zero open faults are described on this page; one more is kept only in the fault store.**"
    );
    assert_eq!(
        faults::count_sentence(1, 57),
        "**One open fault is described on this page; fifty-seven more are kept only in the fault store.**"
    );
    assert_eq!(
        faults::count_sentence(121, 0),
        "**A hundred and twenty-one open faults are described on this page; zero more are kept only in the fault store.**"
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
