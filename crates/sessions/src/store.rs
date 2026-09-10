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
const SESSIONS_SCHEMA_VERSION: i64 = 3;

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

/// The other terminals open on the same work, as the register holds it — one
/// killed without closing stays here. **THE QUESTION IS THE REPOSITORY, NOT
/// THE PATH**; a path no repository claims is compared as itself.
pub fn others_in_the_tree<'a>(
    rows: &'a [TerminalRow],
    mine: &str,
    here: &str,
    repository_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<&'a TerminalRow> {
    let ours = repository_of(here);
    rows.iter()
        .filter(|row| row.is_open() && row.tty != mine)
        .filter(
            |row| match (ours.as_deref(), repository_of(&row.worktree)) {
                (Some(ours), Some(theirs)) => ours == theirs,
                _ => row.worktree == here,
            },
        )
        .collect()
}

/// What one flow was judged to deserve for one event: started, held back with
/// the single reason that held it, or broken on the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub event_id: i64,
    pub flow: String,
    pub verdict: String,
    pub why: Option<String>,
    pub run_id: Option<String>,
    pub decided_at: i64,
}

/// The three words a verdict is written with.
pub const DEFERRED: &str = "deferred";
pub const ACTED: &str = "acted";
pub const BROKE: &str = "broke";

/// A terminal somebody else opened, and the name it goes by to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub tty: String,
    /// The descriptor id of whoever keeps it.
    pub keeper: String,
    pub handle: String,
    /// The variable the handle was read from, so a reader can check it.
    pub named_by: String,
    pub seen_at: i64,
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
                 ON terminal_events (session_id, id);
             CREATE TABLE IF NOT EXISTS event_verdicts (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_id INTEGER NOT NULL,
                 flow TEXT NOT NULL,
                 verdict TEXT NOT NULL,
                 why TEXT,
                 run_id TEXT,
                 decided_at INTEGER NOT NULL
             );
             CREATE UNIQUE INDEX IF NOT EXISTS event_verdicts_once
                 ON event_verdicts (event_id, flow);
             CREATE TABLE IF NOT EXISTS keepers (
                 tty TEXT PRIMARY KEY,
                 keeper TEXT NOT NULL,
                 handle TEXT NOT NULL,
                 named_by TEXT NOT NULL,
                 seen_at INTEGER NOT NULL
             );",
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

    /// Who keeps this terminal, and under what name it is known to them.
    ///
    /// **A HOOK RUNS INSIDE THE SESSION**, so the name comes from the session's
    /// own environment rather than from a guess made outside it. Written at
    /// every arrival: a terminal reopened by another keeper is not the one that
    /// was there before.
    pub fn remember_keeper(&self, kept: &Kept) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO keepers (tty, keeper, handle, named_by, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(tty) DO UPDATE SET
                 keeper = excluded.keeper,
                 handle = excluded.handle,
                 named_by = excluded.named_by,
                 seen_at = excluded.seen_at",
            rusqlite::params![
                kept.tty,
                kept.keeper,
                kept.handle,
                kept.named_by,
                kept.seen_at
            ],
        )?;
        Ok(())
    }

    /// Who keeps that terminal, or nothing where nobody has said.
    pub fn keeper_of(&self, tty: &str) -> Result<Option<Kept>, SessionError> {
        let mut asked = self
            .connection
            .prepare("SELECT tty, keeper, handle, named_by, seen_at FROM keepers WHERE tty = ?1")?;
        let mut rows = asked.query([tty])?;
        match rows.next()? {
            Some(row) => Ok(Some(Kept {
                tty: row.get(0)?,
                keeper: row.get(1)?,
                handle: row.get(2)?,
                named_by: row.get(3)?,
                seen_at: row.get(4)?,
            })),
            None => Ok(None),
        }
    }

    /// Writes the event and gives back the number it was filed under, so
    /// whatever starts because of it can point at the fact that started it.
    pub fn record_event(&self, event: &TerminalEvent) -> Result<i64, SessionError> {
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
        Ok(self.connection.last_insert_rowid())
    }

    /// What one flow was judged to deserve for one event.
    ///
    /// **EVERY EVALUATION LEAVES A ROW, THE REFUSALS INCLUDED.** A guard that
    /// declines in silence cannot be told from one that is broken: the relay
    /// this replaces declined 2,803 times out of 2,834 and left no trace.
    pub fn record_verdict(&self, verdict: &Verdict) -> Result<bool, SessionError> {
        let written = self.connection.execute(
            "INSERT OR IGNORE INTO event_verdicts
                 (event_id, flow, verdict, why, run_id, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                verdict.event_id,
                verdict.flow,
                verdict.verdict,
                verdict.why,
                verdict.run_id,
                verdict.decided_at,
            ],
        )?;
        Ok(written > 0)
    }

    /// Whether this event has already been judged for this flow. **The same
    /// event replayed starts nothing a second time**: a hook called twice by a
    /// command line that retries would otherwise run the work twice.
    pub fn already_judged(&self, event_id: i64, flow: &str) -> Result<bool, SessionError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM event_verdicts WHERE event_id = ?1 AND flow = ?2",
            params![event_id, flow],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Every verdict written for one event, newest last.
    pub fn verdicts_for(&self, event_id: i64) -> Result<Vec<Verdict>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT event_id, flow, verdict, why, run_id, decided_at
             FROM event_verdicts WHERE event_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![event_id], |row| {
            Ok(Verdict {
                event_id: row.get(0)?,
                flow: row.get(1)?,
                verdict: row.get(2)?,
                why: row.get(3)?,
                run_id: row.get(4)?,
                decided_at: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
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
    /// The trees Sailor saw a terminal opened in, most recent first, each once.
    pub fn trees_worked_in(&self) -> Result<Vec<PathBuf>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT worktree FROM terminals GROUP BY worktree ORDER BY MAX(opened_at) DESC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut trees = Vec::new();
        for tree in rows {
            trees.push(PathBuf::from(tree?));
        }
        Ok(trees)
    }

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
