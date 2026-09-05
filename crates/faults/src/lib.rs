//! Faults met while building Sailor, as data.
//!
//! The store assigns the number, so two branches cannot pick the same one.
//!
//! Status stays prose, not an enum: the nuance says which half of the cure is
//! done. [`Fault::still_open`] reads it.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file, next to the ledger.
pub const FAULTS_FILE: &str = "faults.db";

/// The shape this code expects. Independent of the ledger's projections, for
/// the reason written in `Cargo.toml`.
const FAULTS_SCHEMA_VERSION: i64 = 1;

pub enum FaultError {
    Database(rusqlite::Error),
    NoDirectory(String),
    UnsupportedSchema(i64),
    Unknown(i64),
    CannotCrossTheTable(String),
}

/// **`.expect()` PRINTS THE `Debug`, NOT THE `Display`.** With a derived one,
/// every red test showed the fields and hid the sentence this type exists to
/// say. Delegating costs nothing and makes the prose visible where it matters.
impl std::fmt::Debug for FaultError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, out)
    }
}

impl std::fmt::Display for FaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaultError::Database(error) => write!(f, "{error}"),
            FaultError::NoDirectory(what) => write!(f, "{what}"),
            FaultError::UnsupportedSchema(found) => write!(
                f,
                "this fault store is at version {found} and this binary knows \
                 {FAULTS_SCHEMA_VERSION}: it is not broken, it is newer"
            ),
            FaultError::Unknown(number) => write!(f, "fault {number} does not exist"),
            FaultError::CannotCrossTheTable(what) => write!(f, "{what}"),
        }
    }
}

impl std::error::Error for FaultError {}

impl From<rusqlite::Error> for FaultError {
    fn from(error: rusqlite::Error) -> Self {
        FaultError::Database(error)
    }
}

/// A real fault. An entry without `what_would_prevent` is not finished: that
/// column is what separates this from a diary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub number: i64,
    pub happened_on: String,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
}

/// A fault to record: everything except the number, which is not chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub happened_on: String,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
}

/// Where a fault stands, **with a fourth answer for prose nobody taught this**.
///
/// A predicate answering yes or no gives «not open» to a status it does not
/// recognise, which is the same answer it gives to a closed one — and the total
/// drops in the direction that reassures. The fourth answer exists so that an
/// unread status is a refusal instead of a quiet subtraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Open,
    PartlyClosed,
    Closed,
    Unrecognised,
}

/// The words the register writes in that column, in the language the register
/// is written in. Data and not a match, so translating them is one edit and a
/// half-translated row comes out `Unrecognised` instead of vanishing.
const OPEN: &str = "**aperto**";
const PARTLY_CLOSED: &str = "**chiuso in parte**";
const CLOSED: &str = "**chiuso**";

/// The reading, given the prose alone: the store asks it of a status before it
/// belongs to a fault, and the count asks it of one that already does.
///
/// `PARTLY_CLOSED` is tried first because it begins with the word `CLOSED`
/// begins with: asking in the other order would read every half-closed row as
/// closed, and take seven faults out of the tally in one edit.
pub fn standing_of(status: &str) -> Standing {
    let said = status.trim();
    if said.starts_with(PARTLY_CLOSED) {
        Standing::PartlyClosed
    } else if said.starts_with(OPEN) {
        Standing::Open
    } else if said.starts_with(CLOSED) {
        Standing::Closed
    } else {
        Standing::Unrecognised
    }
}

impl Fault {
    /// Read from the start of the prose: the nuance after it says which half of
    /// the cure is done, and must not change where the row is counted.
    pub fn standing(&self) -> Standing {
        standing_of(&self.status)
    }

    /// Open until the cure the fault declares is done. Half-closed counts as
    /// open: a middle state says which half is done, it does not take the row
    /// out of the count. An unrecognised status is **not** quietly closed —
    /// it is refused at the door, so it can never reach this question.
    pub fn still_open(&self) -> bool {
        matches!(self.standing(), Standing::Open | Standing::PartlyClosed)
    }

    fn cells(&self) -> [&str; 6] {
        // The number is missing on purpose: it is formatted separately, and it
        // is the only field that does not come from whoever writes.
        [
            &self.happened_on,
            &self.what_happened,
            &self.how_it_showed,
            &self.what_would_prevent,
            &self.status,
            "",
        ]
    }
}

/// A status the count cannot read is refused, rather than counted as closed.
///
/// **«NOT OPEN» AND «NOT UNDERSTOOD» WERE THE SAME ANSWER.** A row whose status
/// this cannot classify left the open tally with nothing failing, and the total
/// moved in the direction that reassures. Refused here, a half-translated or
/// newly-worded status is a red line instead of a quiet subtraction.
fn a_status_the_count_can_read(status: &str) -> Result<(), FaultError> {
    if standing_of(status) != Standing::Unrecognised {
        return Ok(());
    }
    Err(FaultError::CannotCrossTheTable(format!(
        "this status begins with none of «{OPEN}», «{PARTLY_CLOSED}», «{CLOSED}», \
         so the open count cannot read it and would have counted the fault as \
         closed without saying so. Begin with one of them, or teach the reading \
         first and translate afterwards"
    )))
}

/// What a markdown row cannot carry, and so neither can a fault.
///
/// A newline breaks the row into pieces with the wrong number of columns; the
/// separator adds one. Either way [`parse`] drops the row and the fault leaves
/// the register in silence. Refused at the door, not escaped on the way out:
/// the register *is* the table, so a cell no row holds was never an entry.
fn nothing_that_breaks_a_row(cells: &[(&str, &str)]) -> Result<(), FaultError> {
    for (column, text) in cells {
        let wrong = if text.contains('\n') || text.contains('\r') {
            "a newline"
        } else if text.contains(" | ") {
            "the column separator « | »"
        } else {
            continue;
        };
        return Err(FaultError::CannotCrossTheTable(format!(
            "the «{column}» column contains {wrong}, and a table row cannot \
             carry it: rendered, the row would come apart and reading it back \
             would lose the fault without saying so"
        )));
    }
    Ok(())
}

pub struct Faults {
    connection: Connection,
    path: PathBuf,
}

impl Faults {
    /// Next to the ledger, which is the only place that knows where home is.
    /// Copying that path elsewhere is how it drifts.
    pub fn default_path() -> Result<PathBuf, FaultError> {
        ledger::default_directory()
            .map(|directory| directory.join(FAULTS_FILE))
            .ok_or_else(|| {
                FaultError::NoDirectory(
                    "nowhere to keep the faults: neither SAILOR_LEDGER nor HOME is declared"
                        .to_owned(),
                )
            })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, FaultError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FaultError::NoDirectory(format!("creating {}: {error}", parent.display()))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > FAULTS_SCHEMA_VERSION {
            return Err(FaultError::UnsupportedSchema(version));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS faults (
                 number INTEGER PRIMARY KEY,
                 happened_on TEXT NOT NULL,
                 what_happened TEXT NOT NULL,
                 how_it_showed TEXT NOT NULL,
                 what_would_prevent TEXT NOT NULL,
                 status TEXT NOT NULL
             );",
        )?;
        connection.pragma_update(None, "user_version", FAULTS_SCHEMA_VERSION)?;
        Ok(Faults { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Records a fault and assigns it the number.
    ///
    /// `MAX(number) + 1` is computed inside the insert, so two sessions
    /// recording at once get two different numbers. A test cannot give this
    /// guarantee: a test looks at one branch, and branches do not see
    /// each other.
    pub fn record(&self, draft: &Draft) -> Result<Fault, FaultError> {
        nothing_that_breaks_a_row(&[
            ("data", &draft.happened_on),
            ("cosa è successo", &draft.what_happened),
            ("come si è visto", &draft.how_it_showed),
            ("cosa lo impedirebbe", &draft.what_would_prevent),
            ("stato", &draft.status),
        ])?;
        a_status_the_count_can_read(&draft.status)?;
        self.connection.execute(
            "INSERT INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent, status)
             VALUES (
                 (SELECT COALESCE(MAX(number), 0) + 1 FROM faults),
                 ?1, ?2, ?3, ?4, ?5
             )",
            params![
                draft.happened_on,
                draft.what_happened,
                draft.how_it_showed,
                draft.what_would_prevent,
                draft.status,
            ],
        )?;
        let number = self.connection.last_insert_rowid();
        self.get(number)
    }

    /// Puts a fault back with its own number. Only the migration needs this:
    /// the numbers already existed, and changing them would break every
    /// reference other files make to them.
    pub fn restore(&self, fault: &Fault) -> Result<(), FaultError> {
        nothing_that_breaks_a_row(&[
            ("data", &fault.happened_on),
            ("cosa è successo", &fault.what_happened),
            ("come si è visto", &fault.how_it_showed),
            ("cosa lo impedirebbe", &fault.what_would_prevent),
            ("stato", &fault.status),
        ])?;
        a_status_the_count_can_read(&fault.status)?;
        self.connection.execute(
            "INSERT OR REPLACE INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                fault.number,
                fault.happened_on,
                fault.what_happened,
                fault.how_it_showed,
                fault.what_would_prevent,
                fault.status,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, number: i64) -> Result<Fault, FaultError> {
        self.all()?
            .into_iter()
            .find(|fault| fault.number == number)
            .ok_or(FaultError::Unknown(number))
    }

    pub fn all(&self) -> Result<Vec<Fault>, FaultError> {
        let mut statement = self.connection.prepare(
            "SELECT number, happened_on, what_happened, how_it_showed, what_would_prevent, status
             FROM faults ORDER BY number",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Fault {
                number: row.get(0)?,
                happened_on: row.get(1)?,
                what_happened: row.get(2)?,
                how_it_showed: row.get(3)?,
                what_would_prevent: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Changes a fault's status.
    ///
    /// This is the only thing that can change after recording, which is
    /// already too narrow: a fault that bites a third time is worse than one
    /// that bit once, and there is nowhere to write that.
    pub fn set_status(&self, number: i64, status: &str) -> Result<Fault, FaultError> {
        nothing_that_breaks_a_row(&[("stato", status)])?;
        a_status_the_count_can_read(status)?;
        let touched = self.connection.execute(
            "UPDATE faults SET status = ?2 WHERE number = ?1",
            params![number, status],
        )?;
        if touched == 0 {
            return Err(FaultError::Unknown(number));
        }
        self.get(number)
    }

    /// How many stay open, counted rather than copied. There is no second
    /// place to write the number, so there is no second place to get it wrong.
    pub fn still_open(&self) -> Result<usize, FaultError> {
        Ok(self.all()?.iter().filter(|f| f.still_open()).count())
    }

    pub fn next_open(&self) -> Result<Option<Fault>, FaultError> {
        Ok(self.all()?.into_iter().find(Fault::still_open))
    }

    /// Every fault as one document, `fault:<number>`, in the shape the
    /// ledger's ranking takes: what happened, how it showed, what would
    /// prevent it, and where it stands.
    pub fn documents_to_search(&self) -> Result<Vec<(String, String)>, FaultError> {
        Ok(self
            .all()?
            .into_iter()
            .map(|fault| {
                let text = [
                    fault.what_happened,
                    fault.how_it_showed,
                    fault.what_would_prevent,
                    fault.status,
                ]
                .join(" ");
                (format!("fault:{}", fault.number), text)
            })
            .collect())
    }
}

// ── Markdown: one rendering, and one door in ─────────────────────────────

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
            Some(Fault {
                number,
                happened_on: cells[1].trim().to_owned(),
                what_happened: cells[2].trim().to_owned(),
                how_it_showed: cells[3].trim().to_owned(),
                what_would_prevent: cells[4].trim().to_owned(),
                status: cells[5].trim().to_owned(),
            })
        })
        .collect()
}

/// Writes the rows back the way the table wrote them, for whoever reads that way.
/// The document with its rows replaced, and everything around them kept.
///
/// **A DOCUMENT IS NOT ITS TABLE.** Writing `render`'s rows over the file drops
/// the prose around them: only the run of *data* rows is replaced here.
pub fn render_into(document: &str, faults: &[Fault]) -> String {
    let rows = render(faults);
    let lines: Vec<&str> = document.lines().collect();
    let first = lines.iter().position(|line| is_a_data_row(line));
    let Some(first) = first else {
        // No table to replace: the rows go at the end rather than nowhere.
        let mut out = document.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&rows);
        return out;
    };
    let after = lines[first..]
        .iter()
        .position(|line| !is_a_data_row(line))
        .map_or(lines.len(), |offset| first + offset);
    let mut out = String::new();
    for line in &lines[..first] {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&rows);
    for line in &lines[after..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A row of the register: a first cell holding a number.
fn is_a_data_row(line: &str) -> bool {
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
        out.push_str(&format!(
            "| {} | {on} | {what} | {how} | {prevent} | {status} |\n",
            fault.number
        ));
    }
    out
}
