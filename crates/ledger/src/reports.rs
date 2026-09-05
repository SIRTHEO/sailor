//! What a run left behind, written once and read by the run after it.
//!
//! The report is built from rows already written — the steps and the calls —
//! so the window, the beat and the command line all leave the same one, and a
//! run that closes twice does not get a second.

use crate::{Ledger, LedgerError, StoreRecord};
use serde_json::{json, Value};

/// One entry per flow: the report of the last run of that flow which closed.
pub const RUN_REPORTS: &str = "run-reports";

/// The longest excerpt of what a broken step said the report keeps.
const HOW_MUCH_IT_SAID: usize = 400;

impl Ledger {
    /// What a run did, read off its own rows: the steps that went, those that
    /// broke with the class and the head of what they said, the spend, and
    /// whatever the run declared it learnt.
    ///
    /// The last step's `learnt` is the flow's word, not ours: a run that
    /// declares nothing leaves it null rather than having prose invented.
    pub fn report_of_run(&self, run_id: &str) -> Result<Option<Value>, LedgerError> {
        let Some(header) = self.run_header(run_id)? else {
            return Ok(None);
        };
        let steps = self.steps(run_id)?;
        let mut went = Vec::new();
        let mut broke = Vec::new();
        let mut learnt = Value::Null;
        for step in &steps {
            match step.outcome {
                Some(flow::Outcome::Went) => {
                    went.push(Value::String(step.step_id.clone()));
                    if let Some(said) = step.output.as_ref().and_then(|out| out.get("learnt")) {
                        learnt = said.clone();
                    }
                }
                Some(flow::Outcome::Broke) => broke.push(json!({
                    "step": step.step_id,
                    "class": step.failure_class,
                    "said": step.said.as_deref().map(head_of),
                })),
                _ => {}
            }
        }
        let spent = self.spent_in_run(run_id)?;
        Ok(Some(json!({
            "run_id": run_id,
            "flow": header.entity,
            "status": header.status,
            "what_went": went,
            "what_broke": broke,
            "cost_micros": spent.micros,
            "calls": spent.calls,
            "learnt": learnt,
            "closed_at": header.ended_at,
        })))
    }

    /// The report of the last closed run of `flow`, or nothing when that flow
    /// has never closed one here.
    pub fn last_run_report(&self, flow: &str) -> Result<Option<Value>, LedgerError> {
        Ok(self
            .records_in(RUN_REPORTS)?
            .into_iter()
            .find(|record| record.key == flow)
            .map(|record| record.value))
    }

    /// Writes the report of a closed run, once. A run whose report is already
    /// there writes nothing and says so: a resume that finds the run closed
    /// crosses this same point, and a second report would make the run after
    /// read the same work twice.
    pub fn write_run_report(
        &self,
        run_id: &str,
        written_by: &str,
        written_at: i64,
    ) -> Result<bool, LedgerError> {
        let Some(report) = self.report_of_run(run_id)? else {
            return Ok(false);
        };
        let Some(flow) = report.get("flow").and_then(Value::as_str) else {
            return Ok(false);
        };
        if self
            .last_run_report(flow)?
            .and_then(|held| {
                held.get("run_id")
                    .and_then(Value::as_str)
                    .map(|held| held == run_id)
            })
            .unwrap_or(false)
        {
            return Ok(false);
        }
        self.put_record(&StoreRecord {
            collection: RUN_REPORTS.to_owned(),
            key: flow.to_owned(),
            value: report,
            written_by: written_by.to_owned(),
            written_at,
        })?;
        Ok(true)
    }
}

/// The head of what a step said, cut on a character boundary.
fn head_of(said: &str) -> String {
    if said.len() <= HOW_MUCH_IT_SAID {
        return said.to_owned();
    }
    let mut end = HOW_MUCH_IT_SAID;
    while end > 0 && !said.is_char_boundary(end) {
        end -= 1;
    }
    said[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn a_ledger(label: &str) -> (Ledger, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sailor-ledger-reports-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Ledger::open(&dir).expect("a ledger"), dir)
    }

    fn closed_run(ledger: &Ledger, flow: &str, run_id: &str, status: &str) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO runs (run_id, kind, entity, started_by, status, total_cost_micros, started_at, ended_at)
                 VALUES (?1, 'flow', ?2, 'test', ?3, 0, 10, 20)",
                params![run_id, flow, status],
            )
            .expect("a run row");
    }

    fn step(ledger: &Ledger, run_id: &str, step_id: &str, outcome: &str, said: Option<&str>) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO steps (run_id, step_id, attempt, epoch, deps, input_digest, input,
                     gates, outcome, said, started_at)
                 VALUES (?1, ?2, 1, 1, '[]', '', '{}', '[]', ?3, ?4, 10)",
                params![run_id, step_id, outcome, said],
            )
            .expect("a step row");
    }

    /// The report is what the run's own rows say: which steps went, which
    /// broke and with what, and the flow it belongs to.
    #[test]
    fn the_report_of_a_run_is_read_off_its_own_rows() {
        let (ledger, _dir) = a_ledger("shape");
        closed_run(&ledger, "un-flusso", "corsa-1", "failed");
        step(&ledger, "corsa-1", "primo", "Went", None);
        step(&ledger, "corsa-1", "secondo", "Broke", Some("è andata male"));

        let report = ledger
            .report_of_run("corsa-1")
            .expect("reading the report")
            .expect("the run is there");

        assert_eq!(report["flow"], "un-flusso");
        assert_eq!(report["status"], "failed");
        assert_eq!(report["what_went"], serde_json::json!(["primo"]));
        assert_eq!(report["what_broke"][0]["step"], "secondo");
        assert_eq!(report["what_broke"][0]["said"], "è andata male");
    }

    /// A run whose report is already written does not get a second: the run
    /// after it would otherwise read the same work twice.
    #[test]
    fn the_report_of_a_run_is_written_once() {
        let (ledger, _dir) = a_ledger("once");
        closed_run(&ledger, "un-flusso", "corsa-1", "complete");
        step(&ledger, "corsa-1", "primo", "Went", None);

        assert!(
            ledger
                .write_run_report("corsa-1", "test", 30)
                .expect("writing"),
            "the first close writes it"
        );
        assert!(
            !ledger
                .write_run_report("corsa-1", "test", 40)
                .expect("writing"),
            "and a second close of the same run writes nothing"
        );
    }

    /// The next run of the same flow reads the last report; a flow that never
    /// closed one reads nothing, which is not the same as one that learnt
    /// nothing.
    #[test]
    fn the_last_report_is_the_one_the_next_run_of_that_flow_reads() {
        let (ledger, dir) = a_ledger("last");
        closed_run(&ledger, "un-flusso", "corsa-1", "complete");
        step(&ledger, "corsa-1", "primo", "Went", None);
        ledger
            .write_run_report("corsa-1", "test", 30)
            .expect("writing");
        closed_run(&ledger, "un-flusso", "corsa-2", "failed");
        step(&ledger, "corsa-2", "primo", "Broke", Some("rotto"));
        ledger
            .write_run_report("corsa-2", "test", 40)
            .expect("writing");

        let held = ledger
            .last_run_report("un-flusso")
            .expect("reading")
            .expect("a report is held");

        assert_eq!(held["run_id"], "corsa-2", "the last one, not the first");
        assert_eq!(
            ledger.last_run_report("un-altro").expect("reading"),
            None,
            "a flow that never closed a run here holds nothing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
