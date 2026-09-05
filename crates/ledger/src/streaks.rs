//! How many runs in a row each flow lost, and which streaks a beat already
//! turned into a fault. The counting is a read over the runs the executor
//! wrote; the memory is a store collection, so it survives the window and the
//! shell that wrote it and both read the same one.

use crate::{Ledger, LedgerError, StoreRecord};
use flow::FailureStreak;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The collection where a beat notes the last failed run it wrote a fault for,
/// one entry per flow.
pub const BEAT_FAULTS: &str = "beat-faults";

/// The closed run status the executor writes when a run failed.
const FAILED: &str = "failed";

impl Ledger {
    /// For every flow whose most recent closed runs all failed, how many did
    /// and which was the last. Runs still open are not looked at: a run with
    /// no outcome yet neither extends a streak nor breaks it.
    pub fn failure_streaks(&self, at_least: usize) -> Result<Vec<FailureStreak>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT entity, run_id, status FROM runs
             WHERE kind = 'flow' AND entity <> '' AND ended_at IS NOT NULL
             ORDER BY entity, started_at DESC, run_id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut streaks: BTreeMap<String, FailureStreak> = BTreeMap::new();
        let mut broken: BTreeSet<String> = BTreeSet::new();
        for row in rows {
            let (flow, run_id, status) = row?;
            if broken.contains(&flow) {
                continue;
            }
            if status != FAILED {
                broken.insert(flow);
                continue;
            }
            streaks
                .entry(flow.clone())
                .and_modify(|streak| streak.length += 1)
                .or_insert(FailureStreak {
                    flow,
                    length: 1,
                    last_failed_run: run_id,
                });
        }
        Ok(streaks
            .into_values()
            .filter(|streak| streak.length >= at_least)
            .collect())
    }

    /// The failed runs some beat already wrote a fault about.
    pub fn faults_written(&self) -> Result<BTreeSet<String>, LedgerError> {
        Ok(self
            .records_in(BEAT_FAULTS)?
            .into_iter()
            .filter_map(|record| match record.value {
                Value::String(run_id) => Some(run_id),
                _ => None,
            })
            .collect())
    }

    /// Notes that a fault about `run_id` of `flow` was asked for, so no beat
    /// asks for it twice.
    pub fn remember_fault_written(
        &self,
        flow: &str,
        run_id: &str,
        written_by: &str,
        written_at: i64,
    ) -> Result<(), LedgerError> {
        self.put_record(&StoreRecord {
            collection: BEAT_FAULTS.to_owned(),
            key: flow.to_owned(),
            value: Value::String(run_id.to_owned()),
            written_by: written_by.to_owned(),
            written_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn a_ledger(label: &str) -> (Ledger, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sailor-ledger-streaks-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Ledger::open(&dir).expect("a ledger"), dir)
    }

    /// One closed run row, written the way the executor writes it.
    fn run(ledger: &Ledger, flow: &str, run_id: &str, status: &str, started_at: i64) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO runs (run_id, kind, entity, started_by, status, total_cost_micros, started_at, ended_at)
                 VALUES (?1, 'flow', ?2, 'test', ?3, 0, ?4, ?4)",
                params![run_id, flow, status, started_at],
            )
            .expect("a run row");
    }

    fn open_run(ledger: &Ledger, flow: &str, run_id: &str, started_at: i64) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO runs (run_id, kind, entity, started_by, status, total_cost_micros, started_at)
                 VALUES (?1, 'flow', ?2, 'test', 'running', 0, ?3)",
                params![run_id, flow, started_at],
            )
            .expect("an open run row");
    }

    #[test]
    fn a_streak_is_the_failed_runs_since_the_last_one_that_did_not_fail() {
        let (ledger, dir) = a_ledger("count");
        run(&ledger, "relay", "relay-1", "failed", 10);
        run(&ledger, "relay", "relay-2", "complete", 20);
        run(&ledger, "relay", "relay-3", "failed", 30);
        run(&ledger, "relay", "relay-4", "failed", 40);
        run(&ledger, "relay", "relay-5", "failed", 50);
        run(&ledger, "notte", "notte-1", "failed", 10);
        run(&ledger, "notte", "notte-2", "failed", 20);
        run(&ledger, "healed", "healed-1", "failed", 10);
        run(&ledger, "healed", "healed-2", "failed", 20);
        run(&ledger, "healed", "healed-3", "failed", 30);
        run(&ledger, "healed", "healed-4", "complete", 40);

        let all = ledger.failure_streaks(1).expect("streaks");
        let three = ledger.failure_streaks(3).expect("streaks");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            all,
            vec![
                FailureStreak {
                    flow: "notte".to_owned(),
                    length: 2,
                    last_failed_run: "notte-2".to_owned(),
                },
                FailureStreak {
                    flow: "relay".to_owned(),
                    length: 3,
                    last_failed_run: "relay-5".to_owned(),
                },
            ]
        );
        assert_eq!(three.len(), 1, "{three:?}");
        assert_eq!(three[0].flow, "relay");
    }

    /// A run still open has no outcome: it does not count as a failure, and it
    /// does not hide the three closed ones behind it either. A stopped run,
    /// or one that hit its cap, is not a failure and ends the streak.
    #[test]
    fn an_open_run_neither_extends_nor_breaks_a_streak_and_a_stop_is_not_a_failure() {
        let (ledger, dir) = a_ledger("open");
        run(&ledger, "relay", "relay-1", "failed", 10);
        run(&ledger, "relay", "relay-2", "failed", 20);
        run(&ledger, "relay", "relay-3", "failed", 30);
        open_run(&ledger, "relay", "relay-4", 40);
        run(&ledger, "capped", "capped-1", "failed", 10);
        run(&ledger, "capped", "capped-2", "failed", 20);
        run(&ledger, "capped", "capped-3", "cap_reached", 30);

        let streaks = ledger.failure_streaks(1).expect("streaks");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(streaks.len(), 1, "{streaks:?}");
        assert_eq!(streaks[0].length, 3);
        assert_eq!(streaks[0].last_failed_run, "relay-3");
    }

    #[test]
    fn what_a_beat_remembers_writing_is_read_back_as_the_run_ids() {
        let (ledger, dir) = a_ledger("memory");
        assert!(ledger.faults_written().expect("an empty memory").is_empty());
        ledger
            .remember_fault_written("relay", "relay-5", "test", 60)
            .expect("remembered");
        ledger
            .remember_fault_written("relay", "relay-8", "test", 90)
            .expect("remembered again");
        ledger
            .remember_fault_written("notte", "notte-3", "test", 70)
            .expect("remembered");
        let written = ledger.faults_written().expect("the memory");
        let entries = ledger.records_in(BEAT_FAULTS).expect("the collection");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        // One entry per flow, the newest run standing for it.
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(written, BTreeSet::from(["notte-3".to_owned(), "relay-8".to_owned()]));
    }
}
