//! The store as it stood when a page was counted from it. A page is frozen at a
//! commit while faults keep opening and closing, so the page is compared with
//! that moment of the store and never with the store of the day it is read.

use crate::{render_open_into, Fault, FaultError, Faults, Happening, Standing};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};

/// The highest fault number, the last change the history held, and the
/// instant in the store's clock. A store keeping no history has no change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stood {
    pub through: i64,
    pub change: Option<i64>,
    pub at: String,
}

/// The store's own clock, to the millisecond, so every instant compares as text.
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

pub(crate) const HISTORY: &str = "store_history";

const ROW: &str = "happened_on, what_happened, how_it_showed, what_would_prevent, status, \
                   standing, happened_on_reading, happened_on_value";

/// Each row of the history is what stood just before one change. The first
/// row says when the history began: nothing before it was written down.
pub(crate) fn keep_the_history(connection: &Connection) -> Result<(), FaultError> {
    let old = |row: &str| {
        ROW.split(", ")
            .map(|column| format!("{row}.{}", column.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let faults_before = old("faults");
    let old_row = old("OLD");
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {HISTORY} (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             at TEXT NOT NULL,
             what TEXT NOT NULL,
             number INTEGER,
             existed INTEGER NOT NULL DEFAULT 1,
             {ROW_TEXT},
             summary TEXT
         );
         INSERT INTO {HISTORY} (at, what) SELECT {NOW}, 'kept'
          WHERE NOT EXISTS (SELECT 1 FROM {HISTORY} WHERE what = 'kept');
         CREATE TRIGGER IF NOT EXISTS a_fault_is_written BEFORE INSERT ON faults BEGIN
             INSERT INTO {HISTORY} (at, what, number, existed, {ROW})
                 SELECT {NOW}, 'fault', NEW.number, 1, {faults_before}
                   FROM faults WHERE faults.number = NEW.number;
             INSERT INTO {HISTORY} (at, what, number, existed)
                 SELECT {NOW}, 'fault', NEW.number, 0
                  WHERE NOT EXISTS (SELECT 1 FROM faults WHERE faults.number = NEW.number);
         END;
         CREATE TRIGGER IF NOT EXISTS a_fault_is_changed AFTER UPDATE ON faults BEGIN
             INSERT INTO {HISTORY} (at, what, number, {ROW})
                 VALUES ({NOW}, 'fault', OLD.number, {old_row});
         END;
         CREATE TRIGGER IF NOT EXISTS a_fault_is_taken_out AFTER DELETE ON faults BEGIN
             INSERT INTO {HISTORY} (at, what, number, {ROW})
                 VALUES ({NOW}, 'fault', OLD.number, {old_row});
         END;
         CREATE TRIGGER IF NOT EXISTS a_summary_is_written BEFORE INSERT ON public_summaries BEGIN
             INSERT INTO {HISTORY} (at, what, number, summary)
                 VALUES ({NOW}, 'summary', NEW.number,
                         (SELECT summary FROM public_summaries WHERE number = NEW.number));
         END;
         CREATE TRIGGER IF NOT EXISTS a_summary_is_changed AFTER UPDATE ON public_summaries BEGIN
             INSERT INTO {HISTORY} (at, what, number, summary)
                 VALUES ({NOW}, 'summary', OLD.number, OLD.summary);
         END;
         CREATE TRIGGER IF NOT EXISTS a_summary_is_taken_out AFTER DELETE ON public_summaries BEGIN
             INSERT INTO {HISTORY} (at, what, number, summary)
                 VALUES ({NOW}, 'summary', OLD.number, OLD.summary);
         END;",
        ROW_TEXT = ROW.replace(", ", " TEXT, ") + " TEXT",
    ))?;
    Ok(())
}

const STOOD_OPENING: &str = "Counted from the fault store through fault ";
const STOOD_CHANGE: &str = ", change ";
const STOOD_CHANGE_END: &str = " of its history";
const STOOD_MIDDLE: &str = ", as it stood at ";

pub fn stood_line(stood: &Stood) -> String {
    let change = stood
        .change
        .map(|change| format!("{STOOD_CHANGE}{change}{STOOD_CHANGE_END}"))
        .unwrap_or_default();
    format!(
        "{STOOD_OPENING}{}{change}{STOOD_MIDDLE}{}.",
        stood.through, stood.at
    )
}

pub fn stood_in(document: &str) -> Option<Stood> {
    document.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(STOOD_OPENING)?;
        let (counted, at) = rest.split_once(STOOD_MIDDLE)?;
        let (through, change) = match counted.split_once(STOOD_CHANGE) {
            Some((through, change)) => (
                through,
                Some(change.strip_suffix(STOOD_CHANGE_END)?.parse().ok()?),
            ),
            None => (counted, None),
        };
        Some(Stood {
            through: through.parse().ok()?,
            change,
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
        if crate::is_the_count_sentence(lines[at - 1]) {
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

/// The page rendered again from `faults`, its stamp included.
pub fn the_page_from(document: &str, faults: &[Fault], stood: &Stood) -> String {
    stood_written_into(&render_open_into(document, faults), stood)
}

/// The first line, counted from one, where the page and a render part ways.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub line: usize,
    pub page: String,
    pub store: String,
}

pub fn first_difference(page: &str, rendered: &str) -> Option<Difference> {
    let (page, rendered): (Vec<&str>, Vec<&str>) =
        (page.lines().collect(), rendered.lines().collect());
    (0..page.len().max(rendered.len()))
        .find(|&at| page.get(at) != rendered.get(at))
        .map(|at| Difference {
            line: at + 1,
            page: page.get(at).unwrap_or(&"").to_string(),
            store: rendered.get(at).unwrap_or(&"").to_string(),
        })
}

/// The numbers of the rows one side holds and the other does not hold alike.
pub fn rows_that_differ(page: &str, rendered: &str) -> Vec<i64> {
    let rows = |text: &str| -> BTreeSet<(i64, String)> {
        text.lines()
            .filter(|line| crate::is_a_data_row(line))
            .filter_map(|line| {
                let number = line.trim().trim_start_matches('|').split('|').next()?;
                Some((number.trim().parse().ok()?, line.trim().to_owned()))
            })
            .collect()
    };
    let (page, rendered) = (rows(page), rows(rendered));
    page.symmetric_difference(&rendered)
        .map(|(number, _)| *number)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What a public page is, held against the store it says it was counted from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    Agrees {
        open_then: usize,
    },
    Differs(Difference),
    /// The history does not reach back to the stamp, so nothing is claimed
    /// about that moment: `rows` differ from a render of the store today.
    CannotTell {
        kept_since: Option<String>,
        rows: Vec<i64>,
    },
}

impl Faults {
    pub fn stood_now(&self) -> Result<Stood, FaultError> {
        let history = if self.the_history {
            format!("(SELECT MAX(seq) FROM {HISTORY})")
        } else {
            "NULL".to_owned()
        };
        let (through, change, at) = self.connection.query_row(
            &format!("SELECT COALESCE(MAX(number), 0), {history}, {NOW} FROM faults"),
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(Stood {
            through,
            change,
            at,
        })
    }

    /// When the history began, if this store keeps one.
    pub fn history_kept_since(&self) -> Result<Option<(i64, String)>, FaultError> {
        if !self.the_history {
            return Ok(None);
        }
        Ok(self
            .connection
            .query_row(
                &format!("SELECT seq, at FROM {HISTORY} WHERE what = 'kept' ORDER BY seq LIMIT 1"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    /// The faults up to `through` as they stood at the stamp's change, or
    /// nothing when the history does not reach back that far.
    pub fn as_it_stood(&self, stood: &Stood) -> Result<Option<Vec<Fault>>, FaultError> {
        let reaches_back = matches!(
            (stood.change, self.history_kept_since()?),
            (Some(change), Some((kept, _))) if kept <= change
        );
        if !reaches_back {
            return Ok(None);
        }
        let read = self.connection.unchecked_transaction()?;
        let mut then: BTreeMap<i64, Fault> = self
            .all()?
            .into_iter()
            .map(|fault| (fault.number, fault))
            .collect();
        let mut undone = BTreeSet::new();
        let mut summaries = BTreeMap::new();
        let mut statement = self.connection.prepare(&format!(
            "SELECT what, number, existed, {ROW}, summary FROM {HISTORY}
              WHERE seq > ?1 AND what IN ('fault', 'summary') ORDER BY seq"
        ))?;
        let mut rows = statement.query(params![stood.change])?;
        while let Some(row) = rows.next()? {
            let (what, number): (String, i64) = (row.get(0)?, row.get(1)?);
            if !undone.insert((what.clone(), number)) {
                continue;
            }
            if what == "summary" {
                summaries.insert(number, row.get::<_, Option<String>>(11)?);
            } else if row.get::<_, i64>(2)? == 0 {
                then.remove(&number);
            } else {
                let text = |at: usize| row.get::<_, String>(at);
                let before = Fault {
                    number,
                    happened_on: text(3)?,
                    happened: Happening::from_columns(&text(9)?, &text(10)?)?,
                    what_happened: text(4)?,
                    how_it_showed: text(5)?,
                    what_would_prevent: text(6)?,
                    status: text(7)?,
                    standing: Standing::from_word(&text(8)?)?,
                    public_summary: None,
                    github_issue: None,
                };
                let now = then.remove(&number);
                then.insert(
                    number,
                    Fault {
                        public_summary: now.as_ref().and_then(|it| it.public_summary.clone()),
                        github_issue: now.and_then(|it| it.github_issue),
                        ..before
                    },
                );
            }
        }
        drop(rows);
        drop(statement);
        read.finish()?;
        for (number, summary) in summaries {
            if let Some(fault) = then.get_mut(&number) {
                fault.public_summary = summary;
            }
        }
        Ok(Some(
            then.into_values()
                .filter(|fault| fault.number <= stood.through)
                .collect(),
        ))
    }

    /// The page against a fresh render of the store as it stood at its stamp.
    pub fn hold_the_page(&self, page: &str, stood: &Stood) -> Result<Held, FaultError> {
        let Some(then) = self.as_it_stood(stood)? else {
            let today = the_page_from(page, &self.all()?, stood);
            return Ok(Held::CannotTell {
                kept_since: self.history_kept_since()?.map(|(_, at)| at),
                rows: rows_that_differ(page, &today),
            });
        };
        Ok(
            match first_difference(page, &the_page_from(page, &then, stood)) {
                Some(difference) => Held::Differs(difference),
                None => Held::Agrees {
                    open_then: then.iter().filter(|fault| fault.still_open()).count(),
                },
            },
        )
    }
}
