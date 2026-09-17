//! The handover as a transaction: one session, the activity it declared its
//! work handed on at, one clear sent, one successor.
//!
//! **EVERY MOVE NAMES THE STATE IT LEAVES**, so two controllers cannot both
//! send the clear. The table is added without raising the schema: an older
//! binary keeps reading the file, where raising would make it refuse it.

use crate::store::{SessionError, Sessions};
use rusqlite::{params, OptionalExtension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The session deposited its mandate and declared its turn finished.
    Finished,
    /// A controller holds the one licence to send the clear.
    Clearing,
    AwaitingSuccessor,
    /// A successor reserved the mandate and has not yet been set going.
    Verifying,
    /// The line that sets the successor going was sent; its first turn is awaited.
    Prompted,
    Resumed,
    Cancelled,
    /// Something may have happened that nobody can tell: nothing is retried.
    RecoveryRequired,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Finished => "finished",
            State::Clearing => "clearing",
            State::AwaitingSuccessor => "awaiting_successor",
            State::Verifying => "verifying",
            State::Prompted => "prompted",
            State::Resumed => "resumed",
            State::Cancelled => "cancelled",
            State::RecoveryRequired => "recovery_required",
        }
    }

    fn parse(text: &str) -> State {
        match text {
            "finished" => State::Finished,
            "clearing" => State::Clearing,
            "awaiting_successor" => State::AwaitingSuccessor,
            "verifying" => State::Verifying,
            "prompted" => State::Prompted,
            "resumed" => State::Resumed,
            "cancelled" => State::Cancelled,
            // A state this binary does not know is not one it may act on.
            _ => State::RecoveryRequired,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    pub id: String,
    pub tty: String,
    pub tree: String,
    pub session: String,
    pub engine: String,
    pub mandate_at: i64,
    /// The session's last event when the handover was declared.
    pub generation: i64,
    pub state: State,
    pub why: Option<String>,
    pub successor: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct NewHandover {
    pub tty: String,
    pub tree: String,
    pub session: String,
    pub engine: String,
    pub mandate_at: i64,
    pub at: i64,
}

/// How long a move may stay unfinished: the typing of one line, and the start
/// of a successor. A prompted successor is at work, which has no deadline.
const DEADLINES: [(State, i64); 3] = [
    (State::Clearing, 60),
    (State::AwaitingSuccessor, 300),
    (State::Verifying, 300),
];

const COLUMNS: &str = "id, tty, tree, session_id, engine, mandate_at, generation, state, why, \
                       successor, created_at, updated_at";

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Handover> {
    Ok(Handover {
        id: row.get(0)?,
        tty: row.get(1)?,
        tree: row.get(2)?,
        session: row.get(3)?,
        engine: row.get(4)?,
        mandate_at: row.get(5)?,
        generation: row.get(6)?,
        state: State::parse(&row.get::<_, String>(7)?),
        why: row.get(8)?,
        successor: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

impl Sessions {
    /// The id of a session's last recorded event: its activity generation.
    pub fn activity_of(&self, session: &str) -> Result<i64, SessionError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(id), 0) FROM terminal_events WHERE session_id = ?1",
            params![session],
            |row| row.get(0),
        )?)
    }

    /// Opens a finished handover, cancelling any earlier one of the same
    /// session still waiting to be sent.
    pub fn open_handover(&self, new: &NewHandover) -> Result<Handover, SessionError> {
        let generation = self.activity_of(&new.session)?;
        let id = format!("{}-{}-{}", new.tty, new.session, new.mandate_at);
        self.connection.execute(
            "UPDATE handovers SET state = 'cancelled', why = 'superseded by a newer mandate',
                 updated_at = ?3
             WHERE tty = ?1 AND session_id = ?2 AND state = 'finished'",
            params![new.tty, new.session, new.at],
        )?;
        self.connection.execute(
            "INSERT OR REPLACE INTO handovers
                 (id, tty, tree, session_id, engine, mandate_at, generation, state, why,
                  successor, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'finished', NULL, NULL, ?8, ?8)",
            params![
                id,
                new.tty,
                new.tree,
                new.session,
                new.engine,
                new.mandate_at,
                generation,
                new.at
            ],
        )?;
        self.handover(&id)?
            .ok_or_else(|| SessionError::NoDirectory(format!("handover {id} was not written")))
    }

    pub fn handover(&self, id: &str) -> Result<Option<Handover>, SessionError> {
        Ok(self
            .connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM handovers WHERE id = ?1"),
                params![id],
                read,
            )
            .optional()?)
    }

    /// The finished handover of one session on one terminal, if one waits.
    pub fn open_for(&self, tty: &str, session: &str) -> Result<Option<Handover>, SessionError> {
        self.latest(
            "tty = ?1 AND session_id = ?2 AND state = 'finished'",
            params![tty, session],
        )
    }

    /// The handover whose clear was sent on this terminal, in this tree.
    pub fn awaiting(&self, tty: &str, tree: &str) -> Result<Option<Handover>, SessionError> {
        self.latest(
            "tty = ?1 AND tree = ?2 AND state = 'awaiting_successor'",
            params![tty, tree],
        )
    }

    /// Moves every handover of a terminal stuck mid-way past its deadline to
    /// recovery, and says how many. Waiting to be declared ready has no
    /// deadline: that is the session still at work.
    pub fn overdue(&self, tty: &str, now: i64) -> Result<usize, SessionError> {
        let mut moved = 0;
        for (state, seconds) in DEADLINES {
            moved += self.connection.execute(
                "UPDATE handovers SET state = 'recovery_required', updated_at = ?3,
                     why = 'stuck in ' || state || ' past its deadline'
                 WHERE tty = ?1 AND state = ?2 AND updated_at < ?3 - ?4",
                params![tty, state.as_str(), now, seconds],
            )?;
        }
        Ok(moved)
    }

    /// Whether a clear was sent on this terminal and no successor holds it yet.
    pub fn awaited_on(&self, tty: &str) -> Result<bool, SessionError> {
        Ok(self
            .latest("tty = ?1 AND state = 'awaiting_successor'", params![tty])?
            .is_some())
    }

    /// The handover a successor holds in one state.
    pub fn successor_in(
        &self,
        successor: &str,
        state: State,
    ) -> Result<Option<Handover>, SessionError> {
        self.latest(
            "successor = ?1 AND state = ?2",
            params![successor, state.as_str()],
        )
    }

    /// Every handover, newest first.
    pub fn handovers(&self) -> Result<Vec<Handover>, SessionError> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {COLUMNS} FROM handovers ORDER BY created_at DESC, id"
        ))?;
        let rows = statement.query_map([], read)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn latest(
        &self,
        condition: &str,
        values: impl rusqlite::Params,
    ) -> Result<Option<Handover>, SessionError> {
        Ok(self
            .connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM handovers WHERE {condition}
                     ORDER BY created_at DESC, updated_at DESC LIMIT 1"
                ),
                values,
                read,
            )
            .optional()?)
    }

    /// One move, only from the state named. `false` is a move somebody else
    /// already made, or one that no longer applies.
    pub fn advance(
        &self,
        id: &str,
        from: State,
        to: State,
        why: &str,
        at: i64,
    ) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE handovers SET state = ?3, why = ?4, updated_at = ?5
             WHERE id = ?1 AND state = ?2",
            params![id, from.as_str(), to.as_str(), why, at],
        )?;
        Ok(changed == 1)
    }

    /// Takes the one licence to send the clear, only while the session's
    /// activity is still the one the checks read.
    pub fn clear_if_still(&self, id: &str, read: i64, at: i64) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE handovers SET state = 'clearing', why = NULL, updated_at = ?3
             WHERE id = ?1 AND state = 'finished'
               AND (SELECT COALESCE(MAX(id), 0) FROM terminal_events
                    WHERE session_id = handovers.session_id) = ?2",
            params![id, read, at],
        )?;
        Ok(changed == 1)
    }

    /// The names of a session's events after one of them, in order.
    pub fn events_after(&self, session: &str, after: i64) -> Result<Vec<String>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT name FROM terminal_events WHERE session_id = ?1 AND id > ?2 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![session, after], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Why a finished handover is still waiting, kept on its row.
    pub fn explain(&self, id: &str, why: &str, at: i64) -> Result<(), SessionError> {
        self.connection.execute(
            "UPDATE handovers SET why = ?2, updated_at = ?3 WHERE id = ?1 AND state = 'finished'",
            params![id, why, at],
        )?;
        Ok(())
    }

    /// The successor takes the handover, once.
    pub fn reserve(&self, id: &str, successor: &str, at: i64) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE handovers SET state = 'verifying', successor = ?2, updated_at = ?3
             WHERE id = ?1 AND state = 'awaiting_successor' AND session_id != ?2",
            params![id, successor, at],
        )?;
        Ok(changed == 1)
    }
}
