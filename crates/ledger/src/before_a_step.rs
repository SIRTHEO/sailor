//! Runs that failed before their first step was ever written.
//!
//! **A READING THAT COUNTS BROKEN STEPS CANNOT SEE THESE.** A run refused at
//! its own input dies with no step to count, so every step-shaped reading calls
//! it silence. One flow failed this way every thirty minutes for a day and a
//! half with nothing saying so.

use crate::{Ledger, LedgerError};

/// A flow whose runs keep dying before they begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillbornRun {
    pub flow: String,
    pub times: u64,
    pub last_at: i64,
    /// What the run was refused for, as the executor put it.
    pub error: Option<String>,
}

impl Ledger {
    /// Every flow whose failed runs wrote no step at all, worst first.
    pub fn runs_that_died_before_a_step(
        &self,
        at_least: u64,
    ) -> Result<Vec<StillbornRun>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT r.entity, COUNT(*) AS times, MAX(r.started_at) AS last_at
             FROM runs r
             WHERE r.status = 'failed'
               AND NOT EXISTS (SELECT 1 FROM steps s WHERE s.run_id = r.run_id)
             GROUP BY r.entity
             HAVING times >= ?1
             ORDER BY times DESC, r.entity",
        )?;
        let found = statement
            .query_map([at_least as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        found
            .into_iter()
            .map(|(flow, times, last_at)| {
                let error = connection
                    .query_row(
                        "SELECT r.error FROM runs r
                         WHERE r.entity = ?1 AND r.status = 'failed'
                           AND NOT EXISTS (SELECT 1 FROM steps s WHERE s.run_id = r.run_id)
                         ORDER BY r.started_at DESC LIMIT 1",
                        [&flow],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .unwrap_or_default()
                    .filter(|said| !said.is_empty());
                Ok(StillbornRun { flow, times, last_at, error })
            })
            .collect()
    }
}
