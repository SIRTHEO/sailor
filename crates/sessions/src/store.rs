//! Where whoever announced themselves is kept.
//!
//! **ITS OWN FILE, BESIDE THE LEDGER AND NOT INSIDE IT.** `state.db` has its
//! own `user_version`, which rises when the run projections change, and putting
//! a second reason to rise into it would have two parallel work-streams each
//! choose "the next one" — the same one.

//! The store of whoever arrives second would then declare itself an unsupported
//! version on a machine where nobody changed anything, **and no check would see
//! it**. Here the version is ours.

//! **THE DETACH SITS ON THE TTY.** `detached_at` lives on the terminal's row
//! and **no opening write touches it**: detaching a window detaches it for
//! whoever opens a session there tomorrow. That is what a person means by
//! "leave this window alone" — not "leave this process alone".

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// The file, beside the ledger's own.
pub const SESSIONS_FILE: &str = "sessions.db";

/// The shape this code expects, **independent of the ledger's projection
/// version**. Raise it together with the columns: fault 24 came from a constant
/// left behind by the migration that should have moved it.
const SESSIONS_SCHEMA_VERSION: i64 = 1;

pub enum SessionError {
    Sqlite(rusqlite::Error),
    /// The file was written by a version we do not know. It is not repaired and
    /// not worked around: it is declared.
    UnsupportedSchema(i64),
    NoDirectory(String),
}

impl fmt::Debug for SessionError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, out)
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite: {error}"),
            Self::UnsupportedSchema(version) => write!(
                formatter,
                "unsupported sessions schema version {version}: this file was written \
                 by a newer version of sessions.db"
            ),
            Self::NoDirectory(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<rusqlite::Error> for SessionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// The tracking anchor, and the only thing that identifies a terminal: **the
/// tty, the worktree and the ancestor**. No product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// The kernel object. It is already the neutral name the system gives a
    /// terminal, and it is the key: two windows never share it.
    pub tty: String,
    /// The worktree whoever announced themselves is working in.
    pub worktree: String,
    /// Who drew the window. **Label only**: printed and recorded, read by no
    /// decision. `None` means "we do not know".
    pub ancestor: Option<String>,
}

/// Whoever announces themselves, with what they have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrival {
    pub anchor: Anchor,
    /// The id the agent carries with it, when it has one.
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub at: i64,
}

/// A terminal's row, as it stands now. This one can be corrected; the event
/// queue behind it cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRow {
    pub tty: String,
    pub worktree: String,
    pub ancestor: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub opened_at: i64,
    /// `None` = a session is still open on this terminal. It can stay `None`
    /// forever: a killed terminal closes nothing, and that is a fact to show,
    /// not to hide.
    pub closed_at: Option<i64>,
    /// `None` = attached. Survives every later opening.
    pub detached_at: Option<i64>,
}

impl TerminalRow {
    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }

    pub fn is_detached(&self) -> bool {
        self.detached_at.is_some()
    }
}

/// Something that happened on a terminal, appended and never rewritten. This is
/// the queue the succession of sessions on one tty is reconstructed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEvent {
    pub tty: String,
    pub session_id: Option<String>,
    pub worktree: Option<String>,
    pub ancestor: Option<String>,
    /// What the fact is called. Comes from the payload (`hook_event_name`) when
    /// there is one, otherwise whoever records it supplies it.
    pub name: String,
    pub transcript_path: Option<String>,
    pub occurred_at: i64,
    /// The payload as it arrived, so that what we do not read today is not
    /// lost.
    pub payload: Option<String>,
}

pub struct Sessions {
    connection: Connection,
    path: PathBuf,
}

impl Sessions {
    /// Where the file lives on this machine: **beside the ledger**, in the
    /// directory `ledger::default_directory()` returns. That rule is never
    /// copied out: copying it is fault 19 — the home written down in two
    /// places, with neither of the two declaring itself the wrong one.
    pub fn default_path() -> Result<PathBuf, SessionError> {
        ledger::default_directory()
            .map(|directory| directory.join(SESSIONS_FILE))
            .ok_or_else(|| {
                SessionError::NoDirectory(
                    "nowhere to keep the sessions: neither SAILOR_LEDGER nor HOME is set"
                        .to_owned(),
                )
            })
    }

    /// Opens the file, creating it if it is not there.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                SessionError::NoDirectory(format!("creating {}: {error}", parent.display()))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SESSIONS_SCHEMA_VERSION {
            return Err(SessionError::UnsupportedSchema(version));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS terminals (
                 tty TEXT PRIMARY KEY,
                 worktree TEXT NOT NULL,
                 ancestor TEXT,
                 session_id TEXT,
                 transcript_path TEXT,
                 opened_at INTEGER NOT NULL,
                 closed_at INTEGER,
                 detached_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS terminal_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 tty TEXT NOT NULL,
                 session_id TEXT,
                 worktree TEXT,
                 ancestor TEXT,
                 name TEXT NOT NULL,
                 transcript_path TEXT,
                 occurred_at INTEGER NOT NULL,
                 payload TEXT
             );
             CREATE INDEX IF NOT EXISTS terminal_events_by_terminal
                 ON terminal_events (tty, id);
             CREATE INDEX IF NOT EXISTS terminal_events_by_session
                 ON terminal_events (session_id, id);",
        )?;
        if version < SESSIONS_SCHEMA_VERSION {
            connection.pragma_update(None, "user_version", SESSIONS_SCHEMA_VERSION)?;
        }
        Ok(Self { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The version this file declares. The tests use it to hold the
    /// independence from the ledger's projection version.
    pub fn schema_version(&self) -> Result<i64, SessionError> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Someone opens a session on a terminal.
    ///
    /// **`detached_at` IS NOT AMONG THE UPDATED COLUMNS**, and that is the
    /// point: a detached window stays detached for the agent that arrives next.
    /// If this statement touched it a detach would last one session — "leave
    /// this process alone", where what was asked is "leave this window alone".
    pub fn open_terminal(&self, arrival: &Arrival) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(tty) DO UPDATE SET
                 worktree = excluded.worktree,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor),
                 session_id = excluded.session_id,
                 transcript_path = COALESCE(excluded.transcript_path, terminals.transcript_path),
                 opened_at = CASE
                     WHEN terminals.session_id IS excluded.session_id THEN terminals.opened_at
                     ELSE excluded.opened_at
                 END,
                 closed_at = NULL",
            params![
                arrival.anchor.tty,
                arrival.anchor.worktree,
                arrival.anchor.ancestor,
                arrival.session_id,
                arrival.transcript_path,
                arrival.at,
            ],
        )?;
        Ok(())
    }

    /// An event arrives from a terminal whose opening nobody announced: the row
    /// is created anyway, with what is known.
    ///
    /// It touches neither `closed_at` — an event reopens nothing — nor
    /// `detached_at`, for the reason above.
    pub fn remember_terminal(&self, arrival: &Arrival) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(tty) DO UPDATE SET
                 worktree = excluded.worktree,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor),
                 session_id = COALESCE(excluded.session_id, terminals.session_id),
                 transcript_path = COALESCE(excluded.transcript_path, terminals.transcript_path)",
            params![
                arrival.anchor.tty,
                arrival.anchor.worktree,
                arrival.anchor.ancestor,
                arrival.session_id,
                arrival.transcript_path,
                arrival.at,
            ],
        )?;
        Ok(())
    }

    pub fn record_event(&self, event: &TerminalEvent) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminal_events
                 (tty, session_id, worktree, ancestor, name, transcript_path, occurred_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.tty,
                event.session_id,
                event.worktree,
                event.ancestor,
                event.name,
                event.transcript_path,
                event.occurred_at,
                event.payload,
            ],
        )?;
        Ok(())
    }

    /// Closes the open row on a tty. Returns `false` when there was none: a
    /// close that closed nothing is said out loud, not faked.
    pub fn close_terminal(&self, tty: &str, at: i64) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE terminals SET closed_at = ?2 WHERE tty = ?1 AND closed_at IS NULL",
            params![tty, at],
        )?;
        Ok(changed > 0)
    }

    /// Detaches a terminal. If we did not know it, it is recorded detached: a
    /// detach lost because nobody had announced themselves yet is a detach that
    /// did nothing.
    pub fn detach(&self, anchor: &Anchor, at: i64) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at,
                  detached_at)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?4, ?4)
             ON CONFLICT(tty) DO UPDATE SET
                 detached_at = ?4,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor)",
            params![anchor.tty, anchor.worktree, anchor.ancestor, at],
        )?;
        Ok(())
    }

    /// Reattaches. Returns `false` when that tty was not detached.
    pub fn attach(&self, tty: &str) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE terminals SET detached_at = NULL WHERE tty = ?1 AND detached_at IS NOT NULL",
            params![tty],
        )?;
        Ok(changed > 0)
    }

    pub fn terminal(&self, tty: &str) -> Result<Option<TerminalRow>, SessionError> {
        Ok(self
            .connection
            .query_row(
                "SELECT tty, worktree, ancestor, session_id, transcript_path, opened_at,
                        closed_at, detached_at
                 FROM terminals WHERE tty = ?1",
                params![tty],
                read_terminal,
            )
            .optional()?)
    }

    pub fn terminals(&self) -> Result<Vec<TerminalRow>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT tty, worktree, ancestor, session_id, transcript_path, opened_at,
                    closed_at, detached_at
             FROM terminals ORDER BY tty",
        )?;
        let rows = statement
            .query_map([], read_terminal)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// A terminal's events, in the order they arrived.
    pub fn events_on(&self, tty: &str) -> Result<Vec<TerminalEvent>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT tty, session_id, worktree, ancestor, name, transcript_path, occurred_at,
                    payload
             FROM terminal_events WHERE tty = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![tty], read_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Which sessions followed one another on a terminal, in the order they
    /// were seen. The `terminals` row carries only the last: the succession is
    /// asked of the queue, which is never rewritten.
    pub fn sessions_on(&self, tty: &str) -> Result<Vec<String>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT session_id FROM terminal_events
             WHERE tty = ?1 AND session_id IS NOT NULL
             GROUP BY session_id ORDER BY MIN(id)",
        )?;
        let rows = statement
            .query_map(params![tty], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn read_terminal(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalRow> {
    Ok(TerminalRow {
        tty: row.get(0)?,
        worktree: row.get(1)?,
        ancestor: row.get(2)?,
        session_id: row.get(3)?,
        transcript_path: row.get(4)?,
        opened_at: row.get(5)?,
        closed_at: row.get(6)?,
        detached_at: row.get(7)?,
    })
}

fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalEvent> {
    Ok(TerminalEvent {
        tty: row.get(0)?,
        session_id: row.get(1)?,
        worktree: row.get(2)?,
        ancestor: row.get(3)?,
        name: row.get(4)?,
        transcript_path: row.get(5)?,
        occurred_at: row.get(6)?,
        payload: row.get(7)?,
    })
}
