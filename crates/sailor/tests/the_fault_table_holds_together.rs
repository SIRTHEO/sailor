//! The fault table holds together on its own: numbers with no holes and no
//! duplicates, every entry complete, and the counts written in prose equal to
//! the real ones.
//!
//! **WHY IT EXISTS, AND WHY IT IS NOT FUSSINESS.** Two sessions wrote into the
//! file in the same minute and **two fault 27s and two fault 28s** were born:
//! four rows, two numbers. Nobody noticed, because a document has no compiler.
//! And the prose counts were wrong **in four places out of four**: a number
//! copied by hand diverges, and this table is the source all four claimed to
//! come from. Here the number is obtained by counting, and the prose must say
//! the same.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("the crate lives in <root>/crates/sailor")
        .to_path_buf()
}

struct Fault {
    number: usize,
    cells: Vec<String>,
    standing: faults::Standing,
}

impl Fault {
    /// A fault counts as open until the repair it declares is done.
    ///
    /// **«CLOSED IN PART» IS OPEN, AND THE COUNT MUST SAY SO.** A middle state
    /// tells which half is done; it does not take a row out of the tally. A
    /// reader of «eleven open» believes eleven remain, when twelve do, and the
    /// direction of that error is never random — it is always the reassuring
    /// one, which is why the rule lives here and not in the head of whoever
    /// updates the prose. The reading itself lives in the crate: a second copy
    /// of it here would drift from the one the store counts with.
    fn still_open(&self) -> bool {
        self.standing.still_open()
    }
}

/// The register as the repository carries it.
fn register() -> String {
    let path = repository_root().join("docs/faults-encountered.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The rows of the table, read from the document.
///
/// **THIS READING IS BLIND TO A BLANK LINE INSIDE THE TABLE**, worth knowing
/// before trusting what this test claims. It skips every line not starting with
/// `|`, so a hole between the rows fells nothing: the faults above and below
/// stay correctly numbered and the table goes on «holding together». Closing
/// that gap means to stop filtering and measure the block instead: from the
/// first line starting with `|` to the last, every line between must be a row.
/// The columns the header declares: number, date, three questions, standing.
const COLUMNS: usize = 6;

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
                // **ONE READING, AND IT LIVES IN THE CRATE.** This test kept
                // its own — `contains` against the crate's `starts_with` — and
                // two hand-written readings of one column drift apart: fault 57
                // between a test and the thing it tests. It asks instead, and
                // in exchange it guards what the crate cannot guard alone: that
                // **no row comes out unrecognised**.
                standing: faults::standing_of(&status),
                cells,
            })
        })
        .collect()
}

fn faults() -> Vec<Fault> {
    let rows = faults_in(&register());
    workspace::measured(rows.len(), "rows of the fault table read");
    rows
}

/// Three rows in the shape the register writes them, one per standing.
///
/// **A JUDGE OF THE READING MUST NOT DEPEND ON THE DAY'S REGISTER.** Against
/// the real file the reading is measured on whatever rows happen to be there,
/// and the day no row is closed in part the check measures nothing while
/// staying green.
const A_TABLE_OF_THREE: &str = "\
| # | date | what happened | how it showed | what would have prevented it | status |
|---|---|---|---|---|---|
| 1 | 01/09 | a step called a failure a success | reading the ledger by hand | a non-zero exit breaks the step | **open** |
| 2 | 02/09 | a drawing painted in the colour behind it | a picture of the screen | the screen as an oracle | **closed** on 03/09 |
| 3 | 03/09 | a count that reassured instead of measuring | counting the rows by hand | this table | **closed in part** on 04/09, the measure still to make |
";

/// No repeated number, no hole: the test that would have caught the collision.
#[test]
fn every_fault_has_its_own_number_and_none_is_missing() {
    let faults = faults();
    assert!(faults.len() >= 25, "the table emptied: {}", faults.len());

    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    for fault in &faults {
        *seen.entry(fault.number).or_default() += 1;
    }

    let twice: Vec<usize> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(number, _)| *number)
        .collect();
    assert!(
        twice.is_empty(),
        "two different faults carry the same number: {twice:?}. It happens when \
         two sessions write into the file in the same minute, and nobody notices"
    );

    let missing: Vec<usize> = (1..=faults.len())
        .filter(|n| !seen.contains_key(n))
        .collect();
    assert!(
        missing.is_empty(),
        "numbers skipped: {missing:?}. A hole means a row was taken out without \
         renumbering, and the references other documents make point at nothing"
    );
}

/// **The document has no door to refuse at, so it is guarded here.** The store
/// turns away a status the count cannot read; a markdown table takes anything
/// typed into it, and an unreadable status leaves the open tally in silence.
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
/// A status quoting `||` unescaped split its row into eight columns on the
/// page, and every guard stayed green: the number is the first cell and the
/// standing the last, and a split between them leaves both where they were.
#[test]
fn no_row_carries_more_columns_than_the_header_does() {
    let text = register();
    let rows: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .collect();
    assert!(rows.len() > 100, "only {} rows read: the filter is not looking", rows.len());
    let wide: Vec<&&str> = rows
        .iter()
        .filter(|row| columns_a_reader_sees(row) != COLUMNS)
        .collect();
    assert!(
        wide.is_empty(),
        "a row a reader sees with the wrong number of columns; escape the `|` inside it as `\\|`: {:?}",
        wide.iter().map(|row| &row[..80.min(row.len())]).collect::<Vec<_>>()
    );
}

#[test]
fn every_row_says_where_it_stands_in_words_the_count_can_read() {
    let unread: Vec<usize> = faults()
        .iter()
        .filter(|fault| fault.standing == faults::Standing::Unrecognised)
        .map(|fault| fault.number)
        .collect();

    assert!(
        unread.is_empty(),
        "the status of {unread:?} begins with none of the markers the count can \
         read, so those faults have already left the open tally with nothing \
         saying so. It is the defect the store refuses at the door; here the \
         document is watched, which has no door"
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
         take the row out of the count: reading it as closed lowers the tally \
         in the one direction nobody checks"
    );
}

/// The half-done repair is the real case: translating the column one row at a
/// time would drop the open tally with every row translated.
#[test]
fn a_marker_translated_halfway_leaves_the_count_instead_of_lowering_it() {
    let table = A_TABLE_OF_THREE.replace("**closed in part**", "**chiuso in parte**");
    let rows = faults_in(&table);

    let unread: Vec<usize> = rows
        .iter()
        .filter(|fault| fault.standing == faults::Standing::Unrecognised)
        .map(|fault| fault.number)
        .collect();

    assert_eq!(
        unread,
        vec![3],
        "a marker the reading was never taught must come out unrecognised, not \
         closed: the direction of that error is the reassuring one, and the row \
         would leave the tally with nothing failing"
    );
    assert_eq!(
        faults::standing_of("**aperto** and the rest of the sentence"),
        faults::Standing::Unrecognised,
        "the same holds for a marker left behind by the translation"
    );
}

/// **AN ENTRY WITH NO «WHAT WOULD HAVE PREVENTED IT» IS NOT FINISHED**, as the
/// file says in its own header. A fault with no sequel is a diary, which is
/// exactly what that file declares it is not.
#[test]
fn no_fault_is_left_without_the_check_that_would_have_stopped_it() {
    for fault in faults() {
        assert_eq!(
            fault.cells.len(),
            6,
            "fault {} has not six columns: {:?}",
            fault.number,
            fault.cells
        );
        for (column, name) in [
            "number",
            "date",
            "what happened",
            "how it showed",
            "what would have prevented it",
            "status",
        ]
        .iter()
        .enumerate()
        .map(|(index, name)| (index, *name))
        {
            assert!(
                !fault.cells[column].is_empty(),
                "fault {} has «{name}» empty",
                fault.number
            );
        }
    }
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

/// The number written in letters, the way the prose under the table writes it.
///
/// **THIS WAS ONCE A LIST OF HAND-WRITTEN NUMBERS**, and every new fault forced
/// it longer. A list that grows with the data is a debt on instalments, and it
/// is its own only source, so a typo in it cannot be seen. The rules instead
/// are three and do not change: below twenty there is no rule and the words are
/// listed; above it, ten and unit joined by a hyphen; a hundred and the rest
/// joined by «and».
fn spelled(number: usize) -> String {
    if number < 20 {
        return IRREGULAR[number].to_string();
    }
    assert!(
        number < 1000,
        "the prose has never written a four-digit number in letters: if it must, \
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

/// The hundreds: a bare hundred takes the article, and what follows it takes
/// «and» — a hundred and twenty-three, never a hundred twenty-three.
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

/// **A TRANSLATOR MUST BE CHECKED.** The old version was a hand-written list:
/// right or wrong, it was still the only source, so a typo could not be seen. A
/// function goes wrong differently — on the joins and on the spellings that are
/// not built from the digit's own word — and those are what this test lists,
/// not every number.
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

/// **THE COUNTS IN THE PROSE TELL THE TRUTH.** Under the table the file writes
/// how many faults are still open, out of how many. Those two numbers can be
/// counted, and while a person copies them by hand they diverge: they were wrong
/// in four documents out of four.
#[test]
fn the_counts_written_in_prose_match_the_table_they_come_from() {
    let faults = faults();
    let open = faults.iter().filter(|fault| fault.still_open()).count();
    let total = faults.len();

    // The whole document is searched rather than the section under the table:
    // a heading is prose somebody may re-word, and a re-worded heading must not
    // turn a true count into a red line.
    let prose = register();

    // The capital goes on the word, not on the asterisk before it: the sentence
    // opens with `**`, and capitalising the first character left both forms
    // identical — the test stayed red over prose that was already right.
    let word = spelled(open);
    let capital = {
        let mut chars = word.chars();
        let first = chars.next().expect("the word is not empty");
        format!("{}{}", first.to_uppercase(), chars.as_str())
    };
    let sentence = format!("**{word} are still open** out of {}", spelled(total));
    let capitalized = format!("**{capital} are still open** out of {}", spelled(total));
    assert!(
        prose.contains(&sentence) || prose.contains(&capitalized),
        "the prose does not say the true count. Counted from the table: {open} \
         open out of {total}, that is «{capitalized}». Change the sentence, not \
         the table"
    );
}
