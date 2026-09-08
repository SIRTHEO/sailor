//! Faults met while building Sailor, as data.
//!
//! The store assigns the number, so two branches cannot pick the same one.
//!
//! Standing and date are a validated vocabulary in columns of their own; the
//! prose whoever wrote them stays beside, untouched.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file, next to the ledger.
pub const FAULTS_FILE: &str = "faults.db";

/// The shape this code expects. Independent of the ledger's projections, for
/// the reason written in `Cargo.toml`.
const FAULTS_SCHEMA_VERSION: i64 = 2;

pub enum FaultError {
    Database(rusqlite::Error),
    NoDirectory(String),
    UnsupportedSchema(i64),
    Unknown(i64),
    CannotCrossTheTable(String),
    NotInTheVocabulary { column: String, found: String },
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
            FaultError::NotInTheVocabulary { column, found } => write!(
                f,
                "the «{column}» column holds «{found}», which is not one of the \
                 words this column is allowed: a value nobody taught this must \
                 be refused, never read as the reassuring one"
            ),
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
///
/// `happened_on` and `status` are the prose as somebody wrote it; `happened`
/// and `standing` are the same two facts in a vocabulary a machine can count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub number: i64,
    pub happened_on: String,
    #[serde(default)]
    pub happened: Happening,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
    #[serde(default)]
    pub standing: Standing,
}

/// A fault to record: everything except the number, which is not chosen.
///
/// `standing` left out means «read it from the prose», which is what every
/// caller did before the column existed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub happened_on: String,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
    #[serde(default)]
    pub standing: Option<Standing>,
}

/// Where a fault stands, **with a fourth answer for what nobody classified**.
///
/// A predicate answering yes or no gives «not open» to a status it does not
/// recognise, which is the same answer it gives to a closed one — and the total
/// drops in the direction that reassures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Standing {
    Open,
    #[serde(rename = "closed-in-part")]
    PartlyClosed,
    Closed,
    #[default]
    Unknown,
}

/// The whole vocabulary. A fifth variant breaks the length here and the match
/// in [`Standing::word`], so it cannot arrive without a word of its own.
pub const EVERY_STANDING: [Standing; 4] = [
    Standing::Open,
    Standing::PartlyClosed,
    Standing::Closed,
    Standing::Unknown,
];

impl Standing {
    /// Closed in part counts as open: a middle state says which half is done,
    /// it does not take the row out of the count. Asked in one place, because a
    /// second hand-written reading of a column drifts from the first.
    pub fn still_open(self) -> bool {
        matches!(self, Standing::Open | Standing::PartlyClosed)
    }

    pub fn word(self) -> &'static str {
        match self {
            Standing::Open => "open",
            Standing::PartlyClosed => "closed-in-part",
            Standing::Closed => "closed",
            Standing::Unknown => "unknown",
        }
    }

    /// A word outside the vocabulary is an error, not a fifth reading: the
    /// column is written by this code and a stranger in it means a broken store.
    pub fn from_word(word: &str) -> Result<Standing, FaultError> {
        EVERY_STANDING
            .into_iter()
            .find(|standing| standing.word() == word)
            .ok_or_else(|| FaultError::NotInTheVocabulary {
                column: "standing".to_owned(),
                found: word.to_owned(),
            })
    }
}

/// When a fault happened, **with the missing year said out loud**.
///
/// The register was written half in `dd/mm` and half in `yyyy-mm-dd`: the year
/// of the first half is not recorded anywhere, so it is not guessed here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "reading", rename_all = "kebab-case")]
pub enum Happening {
    #[serde(rename = "day")]
    On { year: i32, month: u32, day: u32 },
    #[serde(rename = "day-and-month")]
    DayAndMonth { month: u32, day: u32 },
    #[default]
    Unknown,
}

const DAY: &str = "day";
const DAY_AND_MONTH: &str = "day-and-month";
const UNKNOWN_DAY: &str = "unknown";

impl Happening {
    pub fn reading(self) -> &'static str {
        match self {
            Happening::On { .. } => DAY,
            Happening::DayAndMonth { .. } => DAY_AND_MONTH,
            Happening::Unknown => UNKNOWN_DAY,
        }
    }

    /// One format. The year-less form is ISO 8601's own truncated shape, so the
    /// gap is visible in the value and not only in the reading beside it.
    pub fn value(self) -> String {
        match self {
            Happening::On { year, month, day } => format!("{year:04}-{month:02}-{day:02}"),
            Happening::DayAndMonth { month, day } => format!("--{month:02}-{day:02}"),
            Happening::Unknown => String::new(),
        }
    }

    /// Reads the prose the register holds: `yyyy-mm-dd` and `dd/mm`, and
    /// nothing else becomes a date by force.
    pub fn read(prose: &str) -> Happening {
        let said = prose.trim();
        let number = |text: &str, digits: usize| -> Option<u32> {
            (text.len() == digits && text.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.parse().ok())
                .flatten()
        };
        let parts: Vec<&str> = said.split('-').collect();
        if let [year, month, day] = parts[..] {
            if let (Some(year), Some(month), Some(day)) = (
                number(year, 4).map(|year| year as i32),
                number(month, 2),
                number(day, 2),
            ) {
                if a_real_day(month, day) {
                    return Happening::On { year, month, day };
                }
            }
        }
        let parts: Vec<&str> = said.split('/').collect();
        if let [day, month] = parts[..] {
            if let (Some(day), Some(month)) = (number(day, 2), number(month, 2)) {
                if a_real_day(month, day) {
                    return Happening::DayAndMonth { month, day };
                }
            }
        }
        Happening::Unknown
    }

    pub fn from_columns(reading: &str, value: &str) -> Result<Happening, FaultError> {
        let refuse = || FaultError::NotInTheVocabulary {
            column: "happened_on_reading".to_owned(),
            found: reading.to_owned(),
        };
        let read = match reading {
            DAY => Happening::read(value),
            DAY_AND_MONTH => Happening::read(&value.replacen("--", "0000-", 1)).flatten_the_year(),
            UNKNOWN_DAY => Happening::Unknown,
            _ => return Err(refuse()),
        };
        if read.reading() != reading {
            return Err(FaultError::NotInTheVocabulary {
                column: "happened_on_value".to_owned(),
                found: value.to_owned(),
            });
        }
        Ok(read)
    }

    fn flatten_the_year(self) -> Happening {
        match self {
            Happening::On { month, day, .. } => Happening::DayAndMonth { month, day },
            other => other,
        }
    }
}

fn a_real_day(month: u32, day: u32) -> bool {
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// The words the register writes in that column, in the language the register
/// is written in. Data and not a match, so translating them is one edit and a
/// half-translated row comes out `Unknown` instead of vanishing.
const OPEN: &str = "**open**";
const PARTLY_CLOSED: &str = "**closed in part**";
const CLOSED: &str = "**closed**";

/// The reading, given the prose alone: the store asks it of a status before it
/// belongs to a fault, and the count asks it of one that already does.
///
/// The closing stars are what keep `CLOSED` from being a prefix of
/// `PARTLY_CLOSED`. Drop them from either marker and the half-closed reading
/// must come first, or every half-closed row reads as closed and leaves the tally.
pub fn standing_of(status: &str) -> Standing {
    let said = status.trim();
    if said.starts_with(PARTLY_CLOSED) {
        Standing::PartlyClosed
    } else if said.starts_with(OPEN) {
        Standing::Open
    } else if said.starts_with(CLOSED) {
        Standing::Closed
    } else {
        Standing::Unknown
    }
}

impl Fault {
    /// An unclassified status is **not** quietly closed: it is refused at the
    /// door unless somebody declared it unknown on purpose.
    pub fn still_open(&self) -> bool {
        self.standing.still_open()
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
    if standing_of(status) != Standing::Unknown {
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

/// **«UNABLE TO OPEN DATABASE FILE» IS NOT A REASON**, and here there are two:
/// a store in WAL mode needs a file beside it even to be read, and a store
/// that is not there at all is a different fact.
fn why_a_reader_was_refused(path: &Path, error: rusqlite::Error) -> FaultError {
    if !path.exists() {
        return FaultError::NoDirectory("there is no register here".to_owned());
    }
    let beside = path.with_extension("readable-check");
    if std::fs::write(&beside, b"").is_err() {
        return FaultError::NoDirectory(
            "it is kept in a directory this reader may not write, and reading a store in WAL \
             mode still asks for a file beside it"
                .to_owned(),
        );
    }
    let _ = std::fs::remove_file(&beside);
    FaultError::Database(error)
}

pub struct Faults {
    connection: Connection,
    path: PathBuf,
    /// A store written before the vocabulary existed still opens read-only,
    /// where nothing may be migrated: its rows are read from the prose.
    the_reading_columns: bool,
}

const THE_READING_COLUMNS: [&str; 3] = ["standing", "happened_on_reading", "happened_on_value"];

fn the_reading_columns_are_there(connection: &Connection) -> Result<bool, FaultError> {
    let mut statement = connection.prepare("PRAGMA table_info(faults)")?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut found = 0;
    for name in names {
        if THE_READING_COLUMNS.contains(&name?.as_str()) {
            found += 1;
        }
    }
    Ok(found == THE_READING_COLUMNS.len())
}

/// Adds the vocabulary columns and fills them from the prose already there.
///
/// The prose is not touched: it is the only record of what somebody meant, and
/// a reading that turns out wrong must be correctable against the source.
fn teach_the_store_the_vocabulary(connection: &Connection) -> Result<(), FaultError> {
    if the_reading_columns_are_there(connection)? {
        return Ok(());
    }
    connection.execute_batch(
        "ALTER TABLE faults ADD COLUMN standing TEXT NOT NULL DEFAULT 'unknown';
         ALTER TABLE faults ADD COLUMN happened_on_reading TEXT NOT NULL DEFAULT 'unknown';
         ALTER TABLE faults ADD COLUMN happened_on_value TEXT NOT NULL DEFAULT '';",
    )?;
    let already: Vec<(i64, String, String)> = {
        let mut statement =
            connection.prepare("SELECT number, happened_on, status FROM faults")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out
    };
    for (number, happened_on, status) in already {
        let happened = Happening::read(&happened_on);
        connection.execute(
            "UPDATE faults
                SET standing = ?2, happened_on_reading = ?3, happened_on_value = ?4
              WHERE number = ?1",
            params![
                number,
                standing_of(&status).word(),
                happened.reading(),
                happened.value(),
            ],
        )?;
    }
    Ok(())
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

    /// The same store, opened **without asking to write it**: a register that
    /// only gets read must be readable where nothing may be written.
    pub fn open_for_reading(path: impl AsRef<Path>) -> Result<Self, FaultError> {
        let path = path.as_ref().to_path_buf();
        let connection =
            Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|error| why_a_reader_was_refused(&path, error))?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| why_a_reader_was_refused(&path, error))?;
        if version > FAULTS_SCHEMA_VERSION {
            return Err(FaultError::UnsupportedSchema(version));
        }
        let the_reading_columns = the_reading_columns_are_there(&connection)?;
        Ok(Faults {
            connection,
            path,
            the_reading_columns,
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
        teach_the_store_the_vocabulary(&connection)?;
        connection.pragma_update(None, "user_version", FAULTS_SCHEMA_VERSION)?;
        Ok(Faults {
            connection,
            path,
            the_reading_columns: true,
        })
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
            ("date", &draft.happened_on),
            ("what happened", &draft.what_happened),
            ("how it showed", &draft.how_it_showed),
            ("what would have prevented it", &draft.what_would_prevent),
            ("status", &draft.status),
        ])?;
        let standing = match draft.standing {
            Some(declared) => declared,
            None => {
                a_status_the_count_can_read(&draft.status)?;
                standing_of(&draft.status)
            }
        };
        let happened = Happening::read(&draft.happened_on);
        self.connection.execute(
            "INSERT INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent,
                  status, standing, happened_on_reading, happened_on_value)
             VALUES (
                 (SELECT COALESCE(MAX(number), 0) + 1 FROM faults),
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
             )",
            params![
                draft.happened_on,
                draft.what_happened,
                draft.how_it_showed,
                draft.what_would_prevent,
                draft.status,
                standing.word(),
                happened.reading(),
                happened.value(),
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
            ("date", &fault.happened_on),
            ("what happened", &fault.what_happened),
            ("how it showed", &fault.how_it_showed),
            ("what would have prevented it", &fault.what_would_prevent),
            ("status", &fault.status),
        ])?;
        a_status_the_count_can_read(&fault.status)?;
        let happened = Happening::read(&fault.happened_on);
        self.connection.execute(
            "INSERT OR REPLACE INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent,
                  status, standing, happened_on_reading, happened_on_value)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                fault.number,
                fault.happened_on,
                fault.what_happened,
                fault.how_it_showed,
                fault.what_would_prevent,
                fault.status,
                standing_of(&fault.status).word(),
                happened.reading(),
                happened.value(),
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
        let columns = if self.the_reading_columns {
            "standing, happened_on_reading, happened_on_value"
        } else {
            "'', '', ''"
        };
        let mut statement = self.connection.prepare(&format!(
            "SELECT number, happened_on, what_happened, how_it_showed, what_would_prevent,
                    status, {columns}
             FROM faults ORDER BY number"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                Fault {
                    number: row.get(0)?,
                    happened_on: row.get(1)?,
                    happened: Happening::Unknown,
                    what_happened: row.get(2)?,
                    how_it_showed: row.get(3)?,
                    what_would_prevent: row.get(4)?,
                    status: row.get(5)?,
                    standing: Standing::Unknown,
                },
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut fault, standing, reading, value) = row?;
            if self.the_reading_columns {
                fault.standing = Standing::from_word(&standing)?;
                fault.happened = Happening::from_columns(&reading, &value)?;
            } else {
                fault.standing = standing_of(&fault.status);
                fault.happened = Happening::read(&fault.happened_on);
            }
            out.push(fault);
        }
        Ok(out)
    }

    /// How many rows sit in each word of the vocabulary, unknown included.
    ///
    /// Open-against-closed is only contable when the third answer is counted
    /// too: without it the unread rows leave the tally the reassuring way.
    pub fn tally(&self) -> Result<std::collections::BTreeMap<Standing, usize>, FaultError> {
        let mut counted = std::collections::BTreeMap::new();
        for standing in EVERY_STANDING {
            counted.insert(standing, 0);
        }
        for fault in self.all()? {
            *counted.entry(fault.standing).or_default() += 1;
        }
        Ok(counted)
    }

    /// Changes a fault's status.
    ///
    /// This is the only thing that can change after recording, which is
    /// already too narrow: a fault that bites a third time is worse than one
    /// that bit once, and there is nowhere to write that.
    pub fn set_status(&self, number: i64, status: &str) -> Result<Fault, FaultError> {
        nothing_that_breaks_a_row(&[("status", status)])?;
        a_status_the_count_can_read(status)?;
        let touched = self.connection.execute(
            "UPDATE faults SET status = ?2, standing = ?3 WHERE number = ?1",
            params![number, status, standing_of(status).word()],
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
        let (on, what) = (as_a_cell(on), as_a_cell(what));
        let (how, prevent, status) = (as_a_cell(how), as_a_cell(prevent), as_a_cell(status));
        out.push_str(&format!(
            "| {} | {on} | {what} | {how} | {prevent} | {status} |\n",
            fault.number
        ));
    }
    out
}
