//! Terminals that stood full, had handed on, and were never emptied.
//!
//! **THESE RUNS ALL ENDED GREEN.** The relay skips the emptying when the
//! screen is never free, and the run completes with nothing done. A session
//! can be owed a handover forty times, lose every one to a compaction, and
//! no reading of failures ever name it.

use crate::{Ledger, LedgerError};

/// A terminal, the times its handover was owed, and the times it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverMissed {
    pub tty: String,
    pub owed: u64,
    pub made: u64,
    pub last_at: i64,
    /// What the terminal answered the last time it was asked to be emptied.
    pub said: Option<String>,
}

impl HandoverMissed {
    pub fn missed(&self) -> u64 {
        self.owed.saturating_sub(self.made)
    }
}

impl Ledger {
    /// Every terminal that stood at `oblige` with a mandate waiting, worst
    /// first. Counted over runs and never over step rows: a step keeps one row
    /// per attempt and per epoch, and joining them multiplies the count.
    pub fn handovers_owed_and_missed(
        &self,
        at_least: u64,
    ) -> Result<Vec<HandoverMissed>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH stood AS (
               SELECT DISTINCT run_id FROM steps
               WHERE step_id = 'measure'
                 AND json_extract(output, '$.state') = 'oblige'
             ), whose AS (
               SELECT run_id, MAX(json_extract(output, '$.tty')) AS tty
               FROM steps WHERE step_id = 'handed_on' GROUP BY run_id
             ), made AS (
               SELECT DISTINCT run_id FROM steps
               WHERE step_id = 'empty' AND outcome = 'Went'
             )
             SELECT w.tty,
                    COUNT(*) AS owed,
                    SUM(CASE WHEN m.run_id IS NOT NULL THEN 1 ELSE 0 END) AS made,
                    MAX(r.started_at) AS last_at
             FROM stood s
             JOIN runs r ON r.run_id = s.run_id
             JOIN whose w ON w.run_id = s.run_id
             LEFT JOIN made m ON m.run_id = s.run_id
             WHERE w.tty IS NOT NULL
             GROUP BY w.tty
             HAVING owed - made >= ?1
             ORDER BY owed - made DESC, w.tty",
        )?;
        let found = statement
            .query_map([at_least as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        found
            .into_iter()
            .map(|(tty, owed, made, last_at)| {
                let said = connection
                    .query_row(
                        "SELECT s.said FROM steps s
                         WHERE s.step_id = 'empty' AND s.said LIKE ?1
                         ORDER BY s.started_at DESC LIMIT 1",
                        [format!("{tty}:%")],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .unwrap_or_default()
                    .filter(|said| !said.is_empty());
                Ok(HandoverMissed { tty, owed, made, last_at, said })
            })
            .collect()
    }
}
