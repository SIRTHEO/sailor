//! The public page of open faults, written whole by this crate. A page is held
//! to a render of the store byte for byte, so no line of it is the page's own:
//! a template the page supplied would copy its forgeries into the render.

use crate::{
    count_sentence, on_the_public_page, public_standing, standing_of, stood_in, stood_line,
    Difference, Fault, Stood, PUBLIC_HEADER,
};

/// Everything above the table, exactly as the page has always said it.
pub const PAGE_OPENING: &str = "# Faults still open\n\
\n\
This page lists known defects in Sailor that are not yet fully repaired and\n\
have a summary written for users, one line each: when the fault was first met,\n\
what goes wrong, and where its repair stands. The full register, with how each\n\
fault showed and what would have prevented it, is kept in the project's own\n\
fault store, and this page is rendered from it by `sailor faults render --open`. To report a new defect, open\n\
an issue with the [bug report form](../.github/ISSUE_TEMPLATE/1_bug_report.yaml);\n\
for a vulnerability, follow [`SECURITY.md`](../SECURITY.md) instead.\n\
\n\
";

const SEPARATOR: &str = "|---|---|---|---|";

/// One row as the page writes it, from cells already escaped.
pub fn public_row(number: i64, since: &str, what: &str, status: &str) -> String {
    format!("| {number} | {since} | {what} | {status} |")
}

/// The whole page: the opening, one table, the count sentence and the stamp.
pub fn the_page(faults: &[Fault], stood: &Stood) -> String {
    let rows = crate::render_open(faults);
    let on_the_page = on_the_public_page(faults).len();
    let open = faults.iter().filter(|fault| fault.still_open()).count();
    let sentence = count_sentence(on_the_page, open - on_the_page);
    format!(
        "{PAGE_OPENING}{PUBLIC_HEADER}\n{SEPARATOR}\n{rows}\n{sentence}\n\n{}\n",
        stood_line(stood)
    )
}

/// The first line, counted from one, where two texts part ways, compared as
/// bytes: a line ending, a space or an invisible letter is a difference.
pub fn first_difference(page: &str, rendered: &str) -> Option<Difference> {
    let (page, rendered): (Vec<&str>, Vec<&str>) = (
        page.split_inclusive('\n').collect(),
        rendered.split_inclusive('\n').collect(),
    );
    (0..page.len().max(rendered.len()))
        .find(|&at| page.get(at) != rendered.get(at))
        .map(|at| Difference {
            line: at + 1,
            page: page
                .get(at)
                .unwrap_or(&"")
                .trim_end_matches('\n')
                .to_string(),
            store: rendered
                .get(at)
                .unwrap_or(&"")
                .trim_end_matches('\n')
                .to_string(),
        })
}

/// The cells of a row, split where a reader splits them: at a `|` not escaped.
pub fn row_cells(row: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for letter in row.chars() {
        if letter == '|' && !escaped {
            cells.push(String::new());
        } else if let Some(cell) = cells.last_mut() {
            cell.push(letter);
        }
        escaped = letter == '\\' && !escaped;
    }
    cells.remove(0);
    if cells.last().is_some_and(|cell| cell.is_empty()) {
        cells.pop();
    }
    cells
        .into_iter()
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// A row the render would write again byte for byte, or nothing.
fn a_public_row(line: &str) -> Option<Vec<String>> {
    let cells = row_cells(line);
    let [number, since, what, status] = cells.as_slice() else {
        return None;
    };
    let number: i64 = number.parse().ok()?;
    let written_again = public_row(number, since, what, status);
    let standing = public_standing(standing_of(status));
    let open = standing_of(status).still_open();
    (written_again == line && standing == status && open && !since.is_empty() && !what.is_empty())
        .then_some(cells)
}

/// The page held to the template with no store at hand: the opening, one
/// table of open rows the render would write again, a count sentence true of
/// those rows, one stamp and nothing more. The rows are its cells. A row
/// invented or removed in the render's shape, or a wrong «more», only the
/// store's check can see.
pub fn held_to_the_template(page: &str) -> Result<Vec<Vec<String>>, Difference> {
    let lines: Vec<&str> = page.split_inclusive('\n').collect();
    let named = |at: usize, store: String| Difference {
        line: at + 1,
        page: lines
            .get(at)
            .unwrap_or(&"")
            .trim_end_matches('\n')
            .to_owned(),
        store,
    };
    let opening = format!("{PAGE_OPENING}{PUBLIC_HEADER}\n{SEPARATOR}\n");
    let above: Vec<&str> = opening.split_inclusive('\n').collect();
    if let Some(at) = (0..above.len()).find(|&at| lines.get(at) != above.get(at)) {
        return Err(named(at, above[at].trim_end_matches('\n').to_owned()));
    }
    let mut at = above.len();
    let mut rows = Vec::new();
    while let Some(line) = lines.get(at).filter(|line| **line != "\n") {
        let row = line
            .strip_suffix('\n')
            .and_then(a_public_row)
            .ok_or_else(|| named(at, "a row as the render writes it".to_owned()))?;
        rows.push(row);
        at += 1;
    }
    if lines.get(at) != Some(&"\n") {
        return Err(named(at, String::new()));
    }
    at += 1;
    let count_is_true = lines.get(at).is_some_and(|line| {
        (0..10_000).any(|others| *line == format!("{}\n", count_sentence(rows.len(), others)))
    });
    if !count_is_true {
        return Err(named(at, count_sentence(rows.len(), 0)));
    }
    if lines.get(at + 1) != Some(&"\n") {
        return Err(named(at + 1, String::new()));
    }
    at += 2;
    let stamp = lines.get(at).and_then(|line| {
        stood_in(line).filter(|stood| *line == format!("{}\n", stood_line(stood)))
    });
    let Some(stamp) = stamp else {
        return Err(named(at, "the stamp".to_owned()));
    };
    // No row under a stamp from a kept history reads the same as every fault
    // hidden, and only the store could tell the two apart.
    if rows.is_empty() && stamp.change.is_some() {
        return Err(named(
            at,
            "rows under a stamp from a kept history".to_owned(),
        ));
    }
    if at + 1 < lines.len() {
        return Err(named(at + 1, String::new()));
    }
    Ok(rows)
}
