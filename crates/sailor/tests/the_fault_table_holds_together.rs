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

/// The rows of the table: every line starting with `|` whose first cell is a
/// number. Header and rule are skipped because their first cell is not one.
fn faults_in(text: &str) -> Vec<Fault> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells: Vec<String> = trimmed
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect();
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

#[test]
fn the_page_carries_at_least_one_fault() {
    let rows = faults();
    assert!(
        !rows.is_empty(),
        "no row was read from the page: either nothing is open, and the page \
         should say so without a table, or the reading is not looking"
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

/// The numbers up to nineteen, which follow no rule at all in English.
const IRREGULAR: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];

/// The tens.
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// The number written in letters, the way the sentence under the table writes
/// it: below twenty listed, then ten and unit joined by a hyphen, then a
/// hundred and the rest joined by «and».
fn spelled(number: usize) -> String {
    if number < 20 {
        return IRREGULAR[number].to_string();
    }
    assert!(
        number < 1000,
        "the page has never written a four-digit number in letters: if it must, \
         the rule for thousands belongs here rather than around it"
    );
    if number >= 100 {
        return with_hundreds(number);
    }
    let (ten, unit) = (number / 10, number % 10);
    let tens = TENS[ten];
    match unit {
        0 => tens.to_string(),
        _ => format!("{tens}-{}", IRREGULAR[unit]),
    }
}

/// A bare hundred takes the article, and what follows it takes «and».
fn with_hundreds(number: usize) -> String {
    let (hundred, rest) = (number / 100, number % 100);
    let prefix = match hundred {
        1 => "a hundred".to_string(),
        _ => format!("{} hundred", IRREGULAR[hundred]),
    };
    if rest == 0 {
        return prefix;
    }
    format!("{prefix} and {}", spelled(rest))
}

/// **A TRANSLATOR MUST BE CHECKED**, on the joins and on the spellings that
/// are not built from the digit's own word.
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
        assert_eq!(spelled(number), word, "{number} is written «{word}»");
    }
}

/// The sentence under the table, for a count: `**Fifty-seven faults are still
/// open.**`, with «fault is» for one.
fn the_count_sentence(count: usize) -> String {
    let word = spelled(count);
    let mut letters = word.chars();
    let first = letters.next().expect("the word is not empty");
    let noun = if count == 1 { "fault is" } else { "faults are" };
    format!("**{}{} {noun} still open.**", first.to_uppercase(), letters.as_str())
}

#[test]
fn the_count_sentence_is_written_for_one_and_for_many() {
    assert_eq!(the_count_sentence(1), "**One fault is still open.**");
    assert_eq!(the_count_sentence(57), "**Fifty-seven faults are still open.**");
}

/// **THE COUNT IN THE PROSE TELLS THE TRUTH.** A number copied by hand
/// diverges; here it is counted from the rows above it.
#[test]
fn the_count_sentence_matches_the_rows_of_the_page() {
    let rows = faults().len();
    let sentence = the_count_sentence(rows);
    assert!(
        page().contains(&sentence),
        "the page does not say the true count. Counted from the table: {rows} \
         rows, that is «{sentence}». Change the sentence, not the table"
    );
}
