//! The public fault page holds together on its own: numbers ascending and
//! unique, four full columns per row, every row still open, and the count
//! written under the table equal to the rows above it.
//!
//! The full register lives in Sailor's store; this page is rendered from it by
//! `sailor faults render --open`, and a hand edit that breaks it is red here.

use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

/// The columns the page declares: number, since, what goes wrong, status.
const COLUMNS: usize = 4;

struct Fault {
    number: usize,
    cells: Vec<String>,
    standing: faults::Standing,
}

impl Fault {
    /// «Closed in part» is open: the reading lives in the crate, and a second
    /// copy here would drift from the one the store counts with.
    fn still_open(&self) -> bool {
        self.standing.still_open()
    }
}

fn page() -> String {
    let path = repository_root().join("docs/faults-encountered.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The rows of the table, read by the same reading `sailor faults check`
/// holds the page with: one table, and one run of rows under its header.
fn faults_in(text: &str) -> Vec<Fault> {
    faults::public_table(text)
        .rows
        .into_iter()
        .filter_map(|(_, cells)| {
            let number: usize = cells.first()?.parse().ok()?;
            let status = cells.last().cloned().unwrap_or_default();
            Some(Fault {
                number,
                standing: faults::standing_of(&status),
                cells,
            })
        })
        .collect()
}

fn faults() -> Vec<Fault> {
    let rows = faults_in(&page());
    workspace::measured(rows.len(), "rows of the public fault page read");
    rows
}

/// Rows in the shape the page writes them, one per standing.
///
/// **A JUDGE OF THE READING MUST NOT DEPEND ON THE DAY'S PAGE**: the day no
/// row is closed in part, a check against the real file measures nothing.
const A_TABLE_OF_THREE: &str = "\
| # | since | what goes wrong | status |
|---|---|---|---|
| 1 | 01/09 | a step called a failure a success | **open** |
| 2 | 02/09 | a drawing painted in the colour behind it | **closed** on 03/09 |
| 3 | 03/09 | a count that reassured instead of measuring | **closed in part** on 04/09, the measure still to make |
";

/// A line that opens with `|` outside the one table is a row no render gave.
#[test]
fn no_line_reads_as_a_row_outside_the_table() {
    let text = page();
    let strays = faults::public_table(&text).strays;
    workspace::measured(text.lines().count(), "lines of the public fault page read");
    assert!(
        strays.is_empty(),
        "lines {strays:?} read as table rows outside the table a render writes; \
         render the page again from the store"
    );
}

/// The three shapes a row slipped past the older reading of this page.
#[test]
fn a_row_the_reading_of_the_check_refuses_is_refused_here_too() {
    let spacer = "| — | | | |\n| 998 | 23/09 | Invented. | **open** |\n";
    let hashed = "| #999 | 23/09 | Invented. | **open** |\n";
    let packed = "| | | | |\n|999|23/09|Invented, written without spaces.|**open**|\n";
    for forged in [spacer, hashed, packed] {
        let table = format!("{A_TABLE_OF_THREE}{forged}");
        let read = faults::public_table(&table);
        assert_eq!(read.strays.first(), Some(&6), "{table}");
        assert_eq!(faults_in(&table).len(), 3, "{table}");
    }
}

/// The page may describe no fault, the day no open one has a summary, but it
/// keeps the table the render writes into.
#[test]
fn the_page_keeps_its_table_even_with_no_row_on_it() {
    let text = page();
    assert!(
        text.lines().any(|line| line.trim() == faults::PUBLIC_HEADER),
        "the header of the table is gone, so the next render has nowhere to put \
         its rows and appends them to the end of the page"
    );
}

#[test]
fn the_numbers_ascend_and_none_repeats() {
    let numbers: Vec<usize> = faults().iter().map(|fault| fault.number).collect();
    let out_of_order: Vec<(usize, usize)> = numbers
        .windows(2)
        .filter(|pair| pair[0] >= pair[1])
        .map(|pair| (pair[0], pair[1]))
        .collect();
    assert!(
        out_of_order.is_empty(),
        "the numbers must be strictly ascending, and these pairs are not: \
         {out_of_order:?}. A repeated number is two faults under one name"
    );
}

/// How many columns a Markdown reader sees: a `|` splits, `\|` does not.
fn columns_a_reader_sees(row: &str) -> usize {
    let mut bars: usize = 0;
    let mut escaped = false;
    for letter in row.trim().chars() {
        match letter {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '|' => bars += 1,
            _ => {}
        }
    }
    bars.saturating_sub(1)
}

/// **A CHECK THAT READS THE ENDS OF A ROW PROVES NOTHING ABOUT ITS MIDDLE.**
/// An unescaped `|` splits a row on the page while its number and status stay
/// where they were.
#[test]
fn every_row_has_four_columns_and_none_is_empty() {
    let text = page();
    for fault in faults_in(&text) {
        let row = text
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&format!("| {} |", fault.number)))
            .expect("the row just read");
        assert_eq!(
            columns_a_reader_sees(row),
            COLUMNS,
            "fault {} shows a reader the wrong number of columns; escape a `|` \
             inside a cell as `\\|`",
            fault.number
        );
        for (column, name) in ["number", "since", "what goes wrong", "status"].iter().enumerate() {
            assert!(
                fault.cells.get(column).is_some_and(|cell| !cell.is_empty()),
                "fault {} has «{name}» empty",
                fault.number
            );
        }
    }
}

/// The page lists what is still open. A row closed, or with a status the one
/// reading cannot classify, has no place on it.
#[test]
fn every_row_is_still_open() {
    let not_open: Vec<(usize, faults::Standing)> = faults()
        .iter()
        .filter(|fault| !fault.still_open())
        .map(|fault| (fault.number, fault.standing))
        .collect();
    assert!(
        not_open.is_empty(),
        "these rows are not open by the reading the store counts with: \
         {not_open:?}. Render the page again from the store"
    );
}

/// **THE STATUS PROSE IS THE REGISTER'S.** A public row says where the fault
/// stands and nothing more, in the words the standing is written with.
#[test]
fn every_status_is_the_standing_alone() {
    let wordy: Vec<usize> = faults()
        .iter()
        .filter(|fault| {
            fault.cells.last().map(String::as_str) != Some(faults::public_standing(fault.standing))
        })
        .map(|fault| fault.number)
        .collect();
    assert!(
        wordy.is_empty(),
        "these rows carry more than their standing in the status column: \
         {wordy:?}. Render the page again from the store"
    );
}

/// **«CLOSED IN PART» IS OPEN.** Reading it as closed takes the half-repaired
/// faults out of the tally in one edit, and the error goes the reassuring way.
#[test]
fn a_row_closed_in_part_is_counted_open_and_never_closed() {
    let rows = faults_in(A_TABLE_OF_THREE);
    assert_eq!(rows.len(), 3, "the three rows of the fixture must be read");

    let open: Vec<usize> = rows
        .iter()
        .filter(|fault| fault.still_open())
        .map(|fault| fault.number)
        .collect();

    assert_eq!(
        open,
        vec![1, 3],
        "row 3 is closed in part, which says which half is done and does not \
         take the row out of the count"
    );
}

/// A marker translated halfway must come out unrecognised, never closed.
#[test]
fn a_marker_translated_halfway_leaves_the_count_instead_of_lowering_it() {
    let table = A_TABLE_OF_THREE.replace("**closed in part**", "**chiuso in parte**");
    let rows = faults_in(&table);

    let unread: Vec<usize> = rows
        .iter()
        .filter(|fault| fault.standing == faults::Standing::Unknown)
        .map(|fault| fault.number)
        .collect();

    assert_eq!(
        unread,
        vec![3],
        "a marker the reading was never taught must come out unrecognised, not \
         closed: the row would leave the tally with nothing failing"
    );
    assert_eq!(
        faults::standing_of("**aperto** and the rest of the sentence"),
        faults::Standing::Unknown,
        "the same holds for a marker left behind by the translation"
    );
}

/// **A TRANSLATOR MUST BE CHECKED**, on the joins and on the spellings that
/// are not built from the digit's own word. The page's count is spelled by it.
#[test]
fn the_numbers_are_spelled_the_way_english_spells_them() {
    for (number, word) in [
        (0, "zero"),
        (3, "three"),
        (13, "thirteen"),
        (15, "fifteen"),
        (19, "nineteen"),
        (20, "twenty"),
        (21, "twenty-one"),
        (23, "twenty-three"),
        (28, "twenty-eight"),
        (30, "thirty"),
        (40, "forty"),
        (41, "forty-one"),
        (47, "forty-seven"),
        (50, "fifty"),
        (57, "fifty-seven"),
        (68, "sixty-eight"),
        (80, "eighty"),
        (91, "ninety-one"),
        (100, "a hundred"),
        (101, "a hundred and one"),
        (103, "a hundred and three"),
        (121, "a hundred and twenty-one"),
        (123, "a hundred and twenty-three"),
        (180, "a hundred and eighty"),
        (200, "two hundred"),
        (308, "three hundred and eight"),
    ] {
        assert_eq!(faults::in_words(number), word, "{number} is written «{word}»");
    }
}

/// **THE COUNT IN THE PROSE TELLS THE TRUTH.** The half the page can see is
/// checked here: how many faults it describes. How many stay only in the store
/// is known to the store alone, and the render writes that half.
#[test]
fn the_count_sentence_matches_the_rows_of_the_page() {
    let rows = faults().len();
    let text = page();
    let sentences: Vec<&str> = text
        .lines()
        .filter(|line| faults::is_the_count_sentence(line))
        .collect();
    assert_eq!(
        sentences.len(),
        1,
        "the page must carry exactly one count sentence, and carries {}",
        sentences.len()
    );
    let expected = faults::count_sentence(rows, 0);
    let (described, _) = expected.split_once(';').expect("the sentence has two halves");
    assert!(
        sentences[0].trim().starts_with(described),
        "the page does not say the true count. Counted from the table: {rows} \
         rows, so the sentence must begin «{described}». Render the page again \
         rather than editing the sentence"
    );
}

/// **A PAGE FROZEN AT A COMMIT IS COMPARED WITH THE STORE AS IT STOOD THEN.**
/// Without the line, the journey compares it with a store that kept moving and
/// is red by construction; and no row may be newer than the line says.
#[test]
fn the_page_says_what_it_was_counted_from() {
    let text = page();
    let stood = faults::stood_in(&text).expect(
        "the page does not say which moment of the store it was counted from. Render the page \
         again from the store",
    );
    assert!(
        stood.change.is_some(),
        "the page was counted from a store that kept no history, so no check can tell what \
         stood then. Render the page again with a binary that keeps it"
    );
    let newer: Vec<usize> = faults()
        .iter()
        .map(|fault| fault.number)
        .filter(|number| *number as i64 > stood.through)
        .collect();
    assert!(
        newer.is_empty(),
        "these rows are newer than the fault the page says it was counted through ({}): {newer:?}",
        stood.through
    );
}
