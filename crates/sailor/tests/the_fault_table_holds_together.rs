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

/// Rows read one line at a time, for fixtures that are a table and no page.
fn faults_in(text: &str) -> Vec<Fault> {
    text.lines()
        .filter(|line| line.trim().starts_with('|'))
        .filter_map(|line| as_a_fault(faults::row_cells(line.trim())))
        .collect()
}

fn as_a_fault(cells: Vec<String>) -> Option<Fault> {
    let number: usize = cells.first()?.parse().ok()?;
    let status = cells.last().cloned().unwrap_or_default();
    Some(Fault {
        number,
        standing: faults::standing_of(&status),
        cells,
    })
}

/// The page's rows, once the page is shown to be the crate's template with
/// rows the render would write again byte for byte.
fn faults() -> Vec<Fault> {
    let rows = faults::held_to_the_template(&page())
        .unwrap_or_else(|difference| panic!("the page is not the template: {difference:?}"));
    workspace::measured(rows.len(), "rows of the public fault page read");
    rows.into_iter().filter_map(as_a_fault).collect()
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

/// No store stands in the forge, so the page is held to what the crate
/// writes: its opening, one table of rows written the render's way, a count
/// true of them and one stamp, byte for byte and nothing after.
#[test]
fn the_page_is_the_template_the_crate_writes() {
    let text = page();
    workspace::measured(text.lines().count(), "lines of the public fault page read");
    if let Err(difference) = faults::held_to_the_template(&text) {
        panic!("render the page again from the store: {difference:?}");
    }
}

fn a_public_fault(number: i64, summary: &str) -> faults::Fault {
    faults::Fault {
        number,
        happened_on: "03/09".to_owned(),
        happened: faults::Happening::read("03/09"),
        what_happened: "what happened".to_owned(),
        how_it_showed: "how it showed".to_owned(),
        what_would_prevent: "what would prevent it".to_owned(),
        status: "**open**".to_owned(),
        standing: faults::Standing::Open,
        public_summary: Some(summary.to_owned()),
        github_issue: None,
    }
}

/// Each shape a page took that a reader of rows let through, so that GFM
/// showed a reader a fault the store never held, or hid the ones it holds.
fn forgeries(page: &str) -> Vec<(&'static str, String)> {
    let invented = "999 | 23/09 | Invented. | **open** |";
    let last_row = page
        .lines()
        .rfind(|line| line.starts_with("| 2 |"))
        .expect("the last row");
    let under_the_rows =
        |row: &str| page.replace(&format!("{last_row}\n"), &format!("{last_row}\n{row}\n"));
    let count = page
        .lines()
        .find(|line| faults::is_the_count_sentence(line))
        .expect("the count");
    let stamp = page
        .lines()
        .find(|line| faults::stood_in(line).is_some())
        .expect("the stamp");
    vec![
        ("A1 a row with no leading bar", under_the_rows(invented)),
        (
            "A2 and no trailing bar",
            under_the_rows(invented.trim_end_matches(" |")),
        ),
        (
            "A3 a bar after an invisible letter",
            under_the_rows(&format!("\u{200b}| {invented}")),
        ),
        (
            "A14 an invisible letter before the number",
            under_the_rows(&format!("\u{200b}{invented}")),
        ),
        (
            "A7 a second table in a quote",
            format!(
                "{page}\n> {}\n> |---|---|---|---|\n> | {invented}\n",
                faults::PUBLIC_HEADER
            ),
        ),
        (
            "A8 a second table with no bars at its ends",
            format!("{page}\n# | since | what goes wrong | status\n---|---|---|---\n{invented}\n"),
        ),
        (
            "A11 a table in HTML",
            format!("{page}\n<table><tr><td>999</td><td>Invented.</td></tr></table>\n"),
        ),
        (
            "B1 a second count sentence",
            format!(
                "{page}\n**Thirty open faults are described on this page; zero more are kept \
                 only in the fault store**.\n"
            ),
        ),
        (
            "B2 the count and stamp in a comment, others shown",
            page.replace(&format!("{count}\n"), &format!("<!--\n{count}\n"))
                + &format!(
                    "-->\n**Twenty open faults are described on this page; ninety more are \
                     kept only in the fault store.**\n\n{}\n",
                    stamp.trim_end_matches('.')
                ),
        ),
        (
            "C1 the whole table in a comment, a forged one shown",
            page.replace(
                &format!("{}\n", faults::PUBLIC_HEADER),
                &format!("<!--\n{}\n", faults::PUBLIC_HEADER),
            ) + &format!(
                "-->\n# | since | what goes wrong | status\n---|---|---|---\n{invented}\n\n\
                 **One open fault is described on this page; zero more are kept only in the \
                 fault store.**\n"
            ),
        ),
        (
            "a row with a space after it",
            page.replace(&format!("{last_row}\n"), &format!("{last_row} \n")),
        ),
        (
            "a row written without spaces",
            page.replace(last_row, &last_row.replace(" | ", "|")),
        ),
        (
            "a status the render never writes",
            page.replace(
                last_row,
                &last_row.replace("**open**", "**open** since the start"),
            ),
        ),
        (
            "a closed row",
            page.replace(last_row, &last_row.replace("**open**", "**closed**")),
        ),
        (
            "a count of the rows that is wrong",
            page.replace(
                count,
                &count.replace("Two open faults", "Three open faults"),
            ),
        ),
        ("a page whose lines end in CRLF", page.replace('\n', "\r\n")),
    ]
}

#[test]
fn every_forgery_is_refused_without_a_store() {
    let stood = faults::Stood {
        through: 2,
        change: Some(4),
        at: "2026-09-23T16:12:36.935Z".to_owned(),
    };
    let faults = [
        a_public_fault(1, "The window forgets a flow."),
        a_public_fault(2, "A run stops without a reason."),
    ];
    let page = faults::the_page(&faults, &stood);
    assert_eq!(
        faults::held_to_the_template(&page).map(|rows| rows.len()),
        Ok(2),
        "a page the crate wrote is the template"
    );
    for (what, forged) in forgeries(&page) {
        assert_ne!(forged, page, "the fixture moved: {what}");
        assert!(
            faults::held_to_the_template(&forged).is_err(),
            "{what} passed\n{forged}"
        );
    }
    let nothing_open = faults::the_page(
        &[],
        &faults::Stood {
            through: 0,
            ..stood
        },
    );
    assert!(
        faults::held_to_the_template(&nothing_open).is_err(),
        "a page with no row under a stamp from a kept history passed"
    );
}

/// The page keeps the table the render writes into. The day no open fault
/// has a summary, the template refuses the page, and so every test that reads
/// it through the template; this one and the count of columns read it alone.
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
