//! The store as it stood when a page was counted from it. A page is frozen at a
//! commit while faults keep opening and closing, so the page is compared with
//! that moment of the store and never with the store of the day it is read.

use crate::{in_words, is_a_data_row, is_the_count_sentence, Fault, FaultError, Faults, Standing};
use rusqlite::{params, OptionalExtension};

/// The highest number the store had handed out, and the instant in its clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stood {
    pub through: i64,
    pub at: String,
}

/// The store's own clock, to the millisecond, so every instant compares as text.
pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

const STOOD_OPENING: &str = "Counted from the fault store through fault ";
const STOOD_MIDDLE: &str = ", as it stood at ";

pub fn stood_line(stood: &Stood) -> String {
    format!(
        "{STOOD_OPENING}{}{STOOD_MIDDLE}{}.",
        stood.through, stood.at
    )
}

pub fn stood_in(document: &str) -> Option<Stood> {
    document.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(STOOD_OPENING)?;
        let (through, at) = rest.split_once(STOOD_MIDDLE)?;
        Some(Stood {
            through: through.parse().ok()?,
            at: at.strip_suffix('.')?.to_owned(),
        })
    })
}

/// The document with one stood line, under its count sentence.
pub fn stood_written_into(document: &str, stood: &Stood) -> String {
    let lines: Vec<&str> = document
        .lines()
        .filter(|line| stood_in(line).is_none())
        .collect();
    let mut out = String::new();
    let mut at = 0;
    while at < lines.len() {
        out.push_str(lines[at]);
        out.push('\n');
        at += 1;
        if is_the_count_sentence(lines[at - 1]) {
            out.push('\n');
            out.push_str(&stood_line(stood));
            out.push('\n');
            while at < lines.len() && lines[at].trim().is_empty() {
                at += 1;
            }
            if at < lines.len() {
                out.push('\n');
            }
        }
    }
    out
}

/// Where a public page and the store as it stood part ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    NotInTheStore(i64),
    NotOpenThen(i64),
    CountDiffers { page: usize, store: usize },
    NoCountSentence,
}

/// `then` is the store as it stood: what [`Faults::as_it_stood`] answers.
pub fn how_the_page_drifts(document: &str, then: &[Fault]) -> Vec<Drift> {
    let mut drifts = Vec::new();
    for number in rows_of(document) {
        match then.iter().find(|fault| fault.number == number) {
            None => drifts.push(Drift::NotInTheStore(number)),
            Some(fault) if !fault.still_open() => drifts.push(Drift::NotOpenThen(number)),
            Some(_) => {}
        }
    }
    let open_then = then.iter().filter(|fault| fault.still_open()).count();
    match document
        .lines()
        .find(|line| is_the_count_sentence(line))
        .and_then(counted_in)
    {
        None => drifts.push(Drift::NoCountSentence),
        Some(page) if page != open_then => drifts.push(Drift::CountDiffers {
            page,
            store: open_then,
        }),
        Some(_) => {}
    }
    drifts
}

fn rows_of(document: &str) -> Vec<i64> {
    document
        .lines()
        .filter(|line| is_a_data_row(line))
        .filter_map(|line| {
            line.trim()
                .trim_start_matches('|')
                .split('|')
                .next()?
                .trim()
                .parse()
                .ok()
        })
        .collect()
}

/// Both halves of the sentence, added: the open faults it says the store held.
fn counted_in(sentence: &str) -> Option<usize> {
    let said = sentence.trim().trim_start_matches("**").to_lowercase();
    let (page, store) = said.split_once("; ")?;
    let described = page.split(" open fault").next()?;
    let more = store.split(" more ").next()?;
    Some(number_in_words(described)? + number_in_words(more)?)
}

fn number_in_words(words: &str) -> Option<usize> {
    (0..10_000).find(|number| in_words(*number) == words)
}

impl Faults {
    pub fn stood_now(&self) -> Result<Stood, FaultError> {
        let (through, at) = self.connection.query_row(
            &format!("SELECT COALESCE(MAX(number), 0), {NOW} FROM faults"),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(Stood { through, at })
    }

    /// The faults up to `through`, each with the standing it had at `at`. A
    /// store written only by a binary that kept no changes answers today's.
    pub fn as_it_stood(&self, stood: &Stood) -> Result<Vec<Fault>, FaultError> {
        let mut then = Vec::new();
        for mut fault in self
            .all()?
            .into_iter()
            .filter(|fault| fault.number <= stood.through)
        {
            if let Some(was) = self.standing_before(fault.number, &stood.at)? {
                fault.standing = was;
            }
            then.push(fault);
        }
        Ok(then)
    }

    fn standing_before(&self, number: i64, at: &str) -> Result<Option<Standing>, FaultError> {
        if !self.the_standing_changes {
            return Ok(None);
        }
        let was: Option<String> = self
            .connection
            .query_row(
                "SELECT was FROM standing_changes WHERE number = ?1 AND at > ?2
                  ORDER BY at, rowid LIMIT 1",
                params![number, at],
                |row| row.get(0),
            )
            .optional()?;
        was.map(|word| Standing::from_word(&word)).transpose()
    }

    pub(crate) fn standing_held(&self, number: i64) -> Result<Option<Standing>, FaultError> {
        let held: Option<String> = self
            .connection
            .query_row(
                "SELECT standing FROM faults WHERE number = ?1",
                params![number],
                |row| row.get(0),
            )
            .optional()?;
        held.map(|word| Standing::from_word(&word)).transpose()
    }

    pub(crate) fn write_the_change(
        &self,
        number: i64,
        was: Standing,
        became: Standing,
    ) -> Result<(), FaultError> {
        if was == became {
            return Ok(());
        }
        self.connection.execute(
            &format!(
                "INSERT INTO standing_changes (number, was, became, at) VALUES (?1, ?2, ?3, {NOW})"
            ),
            params![number, was.word(), became.word()],
        )?;
        Ok(())
    }
}
