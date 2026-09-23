//! The fault table as a document: reading one in, and writing one back.

use crate::{standing_of, Fault, Happening, Standing, CLOSED, OPEN, PARTLY_CLOSED};

/// Reads a hand-written fault table.
///
/// It exists for the migration, and then to disprove it: the round-trip test
/// writes the rows back and compares them to the source, which is the only
/// way to know none was lost on the way in.
pub fn parse(markdown: &str) -> Vec<Fault> {
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells: Vec<&str> = trimmed.trim_matches('|').split(" | ").collect();
            if cells.len() != 6 {
                return None;
            }
            let number: i64 = cells[0].trim().parse().ok()?;
            let happened_on = as_prose(cells[1]);
            let status = as_prose(cells[5]);
            Some(Fault {
                number,
                happened: Happening::read(&happened_on),
                happened_on,
                what_happened: as_prose(cells[2]),
                how_it_showed: as_prose(cells[3]),
                what_would_prevent: as_prose(cells[4]),
                standing: standing_of(&status),
                status,
                public_summary: None,
                github_issue: None,
            })
        })
        .collect()
}

/// Escaping a `|` is the writing's business: fault 125 rendered in eight columns.
fn as_a_cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn as_prose(cell: &str) -> String {
    cell.trim().replace("\\|", "|")
}

/// Writes the rows back the way the table wrote them, for whoever reads that way.
/// The document with its rows replaced, and everything around them kept.
///
/// **A DOCUMENT IS NOT ITS TABLE.** Writing `render`'s rows over the file drops
/// the prose around them: only the run of *data* rows is replaced here.
pub fn render_into(document: &str, faults: &[Fault]) -> String {
    rows_replaced(document, &render(faults))
}

/// The public page: its rows and its count sentence replaced, and the prose
/// around them kept as [`render_into`] keeps it. A document with no count
/// sentence gets one at its end.
pub fn render_open_into(document: &str, faults: &[Fault]) -> String {
    let on_the_page = on_the_public_page(faults).len();
    let open = faults.iter().filter(|fault| fault.still_open()).count();
    let sentence = count_sentence(on_the_page, open - on_the_page);
    let with_rows = rows_replaced(document, &render_open(faults));
    let mut out = String::new();
    let mut replaced = false;
    for line in with_rows.lines() {
        if !replaced && is_the_count_sentence(line) {
            out.push_str(&sentence);
            replaced = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !replaced {
        out.push('\n');
        out.push_str(&sentence);
        out.push('\n');
    }
    out
}

/// How the count sentence ends, so a render finds the line it rewrites.
pub const COUNT_SENTENCE_END: &str = "kept only in the fault store.**";

pub fn is_the_count_sentence(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("**") && line.ends_with(COUNT_SENTENCE_END)
}

/// `**Three open faults are described on this page; fifty-five more are kept
/// only in the fault store.**`
pub fn count_sentence(on_the_page: usize, only_in_the_store: usize) -> String {
    let words = in_words(on_the_page);
    let mut letters = words.chars();
    let capital: String = letters.next().map(|first| first.to_uppercase().collect()).unwrap_or_default();
    let page = if on_the_page == 1 { "open fault is" } else { "open faults are" };
    let store = if only_in_the_store == 1 { "is" } else { "are" };
    format!(
        "**{capital}{} {page} described on this page; {} more {store} {COUNT_SENTENCE_END}",
        letters.as_str(),
        in_words(only_in_the_store)
    )
}

/// The faults the public page shows: still open, and given a summary for users.
pub fn on_the_public_page(faults: &[Fault]) -> Vec<(&Fault, &str)> {
    faults
        .iter()
        .filter(|fault| fault.still_open())
        .filter_map(|fault| fault.public_summary.as_deref().map(|summary| (fault, summary)))
        .collect()
}

const BELOW_TWENTY: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen",
    "nineteen",
];

const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A count in English words: tens and units joined by a hyphen, a hundred and
/// the rest by «and». From a thousand on, the digits.
pub fn in_words(number: usize) -> String {
    match number {
        0..=19 => BELOW_TWENTY[number].to_owned(),
        20..=99 => match (TENS[number / 10], number % 10) {
            (tens, 0) => tens.to_owned(),
            (tens, unit) => format!("{tens}-{}", BELOW_TWENTY[unit]),
        },
        100..=999 => {
            let hundreds = match number / 100 {
                1 => "a hundred".to_owned(),
                many => format!("{} hundred", BELOW_TWENTY[many]),
            };
            match number % 100 {
                0 => hundreds,
                rest => format!("{hundreds} and {}", in_words(rest)),
            }
        }
        _ => number.to_string(),
    }
}

fn rows_replaced(document: &str, rows: &str) -> String {
    let lines: Vec<&str> = document.lines().collect();
    let first = lines.iter().position(|line| is_a_data_row(line));
    let Some(first) = first else {
        // An empty table takes its rows under the header's separator.
        if let Some(separator) = lines.iter().position(|line| is_a_separator(line)) {
            return spliced(&lines, separator + 1, separator + 1, rows);
        }
        // No table to replace: the rows go at the end rather than nowhere.
        let mut out = document.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(rows);
        return out;
    };
    let after = lines[first..]
        .iter()
        .position(|line| !is_a_data_row(line))
        .map_or(lines.len(), |offset| first + offset);
    spliced(&lines, first, after, rows)
}

/// The lines before `from`, the rows, then the lines from `to` on.
fn spliced(lines: &[&str], from: usize, to: usize, rows: &str) -> String {
    let mut out = String::new();
    for line in &lines[..from] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(rows);
    for line in &lines[to..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The `|---|---|` line under a table's header.
fn is_a_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.contains('-')
        && trimmed.chars().all(|letter| matches!(letter, '|' | '-' | ':' | ' '))
}

/// A row of the register: a first cell holding a number.
pub fn is_a_data_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    trimmed
        .trim_start_matches('|')
        .split('|')
        .next()
        .is_some_and(|cell| cell.trim().parse::<i64>().is_ok())
}

pub fn render(faults: &[Fault]) -> String {
    let mut out = String::new();
    for fault in faults {
        let [on, what, how, prevent, status, _] = fault.cells();
        let (on, what) = (as_a_cell(on), as_a_cell(what));
        let (how, prevent, status) = (as_a_cell(how), as_a_cell(prevent), as_a_cell(status));
        out.push_str(&format!(
            "| {} | {on} | {what} | {how} | {prevent} | {status} |\n",
            fault.number
        ));
    }
    out
}

/// The header of the public page's table, which a render writes under.
pub const PUBLIC_HEADER: &str = "| # | since | what goes wrong | status |";

/// One row per fault on the public page: `| # | since | what goes wrong | status |`.
/// The status is the standing's marker alone: the prose after it is the register's.
pub fn render_open(faults: &[Fault]) -> String {
    let mut out = String::new();
    for (fault, summary) in on_the_public_page(faults) {
        let since = as_a_cell(&fault.happened_on);
        let what = as_a_cell(summary);
        let status = public_standing(fault.standing);
        out.push_str(&crate::public_row(fault.number, &since, &what, status));
        out.push('\n');
    }
    out
}

/// The only words the public status column holds, read from the standing.
pub fn public_standing(standing: Standing) -> &'static str {
    match standing {
        Standing::Open => OPEN,
        Standing::PartlyClosed => PARTLY_CLOSED,
        Standing::Closed => CLOSED,
        Standing::Unknown => "**unknown**",
    }
}
