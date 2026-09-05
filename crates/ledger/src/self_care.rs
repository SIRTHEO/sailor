//! What a run that looked after the tree left behind: one line, once.
//!
//! Built from rows already written — the run's header and its steps — so the
//! window, the beat and the command line all leave the same line, and a run
//! that closes twice does not get a second.

use crate::{Ledger, LedgerError, StoreRecord};
use serde_json::{json, Value};

/// One entry per closed run of a flow that declared it looks after itself.
pub const SELF_CARE_LINES: &str = "self-care-lines";

/// The three verdicts, as the line keeps them.
const KEEP: &str = "keep";
const DISCARD: &str = "discard";
const CRASH: &str = "crash";

/// The run status the executor writes when every step was reached.
const COMPLETE: &str = "complete";

impl Ledger {
    /// The lines of closed self-care runs, newest first. A tree where nothing
    /// has looked after itself answers with none, which is an answer.
    pub fn self_care_lines(
        &self,
        flow: Option<&str>,
        within_last: usize,
    ) -> Result<Vec<Value>, LedgerError> {
        let mut lines: Vec<Value> = self
            .records_in(SELF_CARE_LINES)?
            .into_iter()
            .map(|record| record.value)
            .filter(|line| {
                flow.is_none_or(|wanted| line.get("flow").and_then(Value::as_str) == Some(wanted))
            })
            .collect();
        lines.sort_by_key(|line| {
            std::cmp::Reverse(line.get("closed_at").and_then(Value::as_i64).unwrap_or(0))
        });
        lines.truncate(within_last);
        Ok(lines)
    }

    /// The line of a closed run, as its own rows tell it.
    ///
    /// The metric and the sentence are the last step's word: a run declaring
    /// neither leaves them null rather than having prose invented. `said` is
    /// the caller's fallback when the flow said nothing.
    pub fn self_care_line(
        &self,
        run_id: &str,
        commit: Option<String>,
        said: Option<&str>,
    ) -> Result<Option<Value>, LedgerError> {
        let Some(header) = self.run_header(run_id)? else {
            return Ok(None);
        };
        let last = self
            .steps(run_id)?
            .into_iter()
            .rfind(|step| step.outcome == Some(flow::Outcome::Went));
        let output = last.and_then(|step| step.output);
        let field = |name: &str| {
            output
                .as_ref()
                .and_then(|output| output.get(name))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let metric = field("metric");
        let sentence = match field("sentence") {
            Value::Null => said.map(Value::from).unwrap_or(Value::Null),
            said_by_the_flow => said_by_the_flow,
        };
        let verdict = match (header.status.as_str(), metric.is_null()) {
            (COMPLETE, false) => KEEP,
            (COMPLETE, true) => DISCARD,
            _ => CRASH,
        };
        Ok(Some(json!({
            "run_id": run_id,
            "flow": header.entity,
            "commit": commit,
            "metric": metric,
            "verdict": verdict,
            "sentence": sentence,
            "closed_at": header.ended_at,
        })))
    }

    /// Writes the line of a closed run, once. A run whose line is already there
    /// writes nothing and says so: a resume that finds the run closed crosses
    /// this same point, and two lines for one run would be counted as two.
    pub fn write_self_care_line(
        &self,
        run_id: &str,
        commit: Option<String>,
        said: Option<&str>,
        written_by: &str,
        written_at: i64,
    ) -> Result<bool, LedgerError> {
        let Some(line) = self.self_care_line(run_id, commit, said)? else {
            return Ok(false);
        };
        if self.read_record(SELF_CARE_LINES, run_id)?.is_some() {
            return Ok(false);
        }
        self.put_record(&StoreRecord {
            collection: SELF_CARE_LINES.to_owned(),
            key: run_id.to_owned(),
            value: line,
            written_by: written_by.to_owned(),
            written_at,
        })?;
        Ok(true)
    }

    /// How many runs of this flow the store holds besides the one named.
    ///
    /// A turn is one recorded run, however it ended. Excluding the asking run
    /// is what makes the n+1th the one that stops.
    pub fn turns_of_flow(&self, flow: &str, apart_from: &str) -> Result<u32, LedgerError> {
        let connection = self.lock()?;
        let taken: i64 = connection.query_row(
            "SELECT COUNT(*) FROM runs
             WHERE kind = 'flow' AND entity = ?1 AND run_id <> ?2",
            rusqlite::params![flow, apart_from],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(taken).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn a_ledger(label: &str) -> (Ledger, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "sailor-ledger-self-care-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Ledger::open(&dir).expect("a ledger"), dir)
    }

    fn closed_run(ledger: &Ledger, flow: &str, run_id: &str, status: &str, ended_at: i64) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO runs (run_id, kind, entity, started_by, status,
                     total_cost_micros, started_at, ended_at)
                 VALUES (?1, 'flow', ?2, 'test', ?3, 0, 10, ?4)",
                params![run_id, flow, status, ended_at],
            )
            .expect("a run row");
    }

    fn step(ledger: &Ledger, run_id: &str, step_id: &str, output: Option<&str>) {
        let connection = ledger.lock().expect("the connection");
        connection
            .execute(
                "INSERT INTO steps (run_id, step_id, attempt, epoch, deps, input_digest,
                     input, gates, outcome, output, started_at)
                 VALUES (?1, ?2, 1, 1, '[]', '', '{}', '[]', 'Went', ?3, 10)",
                params![run_id, step_id, output],
            )
            .expect("a step row");
    }

    /// A run that reached its last step and moved a metric is worth keeping,
    /// and the metric and the sentence are the flow's own words.
    #[test]
    fn a_completed_run_that_moved_a_metric_is_kept() {
        let (ledger, dir) = a_ledger("keep");
        closed_run(&ledger, "cura", "corsa-1", "complete", 20);
        step(&ledger, "corsa-1", "primo", None);
        step(
            &ledger,
            "corsa-1",
            "ultimo",
            Some(r#"{"metric": "warnings: 3", "sentence": "three warnings went"}"#),
        );

        let line = ledger
            .self_care_line("corsa-1", Some("abc123".to_owned()), None)
            .expect("reading the line")
            .expect("the run is there");

        assert_eq!(line["verdict"], "keep");
        assert_eq!(line["metric"], "warnings: 3");
        assert_eq!(line["sentence"], "three warnings went");
        assert_eq!(line["commit"], "abc123");
        assert_eq!(line["closed_at"], 20);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A run that went the whole way and moved nothing is not a failure and is
    /// not a keeper either: its work is discarded.
    #[test]
    fn a_completed_run_that_moved_nothing_is_discarded() {
        let (ledger, dir) = a_ledger("discard");
        closed_run(&ledger, "cura", "corsa-1", "complete", 20);
        step(&ledger, "corsa-1", "ultimo", Some(r#"{"said": "nothing to do"}"#));

        let line = ledger
            .self_care_line("corsa-1", None, None)
            .expect("reading the line")
            .expect("the run is there");

        assert_eq!(line["verdict"], "discard");
        assert_eq!(line["metric"], Value::Null);
        assert_eq!(line["commit"], Value::Null);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A run that failed or stopped short reads `crash`, and the sentence is
    /// the caller's when the flow left none.
    #[test]
    fn a_run_that_never_reached_the_end_reads_crash() {
        let (ledger, dir) = a_ledger("crash");
        closed_run(&ledger, "cura", "corsa-1", "stopped", 20);

        let line = ledger
            .self_care_line("corsa-1", None, Some("it met its wall"))
            .expect("reading the line")
            .expect("the run is there");

        assert_eq!(line["verdict"], "crash");
        assert_eq!(line["sentence"], "it met its wall");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The lines come back newest first, and only for the flow asked about.
    #[test]
    fn the_lines_come_back_newest_first_for_the_flow_asked() {
        let (ledger, dir) = a_ledger("order");
        for (flow, run_id, ended_at) in [
            ("cura", "corsa-1", 20),
            ("cura", "corsa-2", 40),
            ("altro", "corsa-3", 30),
        ] {
            closed_run(&ledger, flow, run_id, "complete", ended_at);
            step(&ledger, run_id, "ultimo", Some(r#"{"metric": "one"}"#));
            ledger
                .write_self_care_line(run_id, None, None, "test", ended_at)
                .expect("writing the line");
        }

        let mine = ledger.self_care_lines(Some("cura"), 50).expect("reading");
        assert_eq!(mine.len(), 2);
        assert_eq!(mine[0]["run_id"], "corsa-2", "newest first");
        assert_eq!(mine[1]["run_id"], "corsa-1");

        let all = ledger.self_care_lines(None, 50).expect("reading");
        assert_eq!(all.len(), 3, "no flow asked for means every flow");
        let one = ledger.self_care_lines(None, 1).expect("reading");
        assert_eq!(one.len(), 1, "the window is a count of lines");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A turn is a run of the same flow, and the run doing the asking is never
    /// one of them: otherwise the first run of a flow would already be its
    /// second.
    #[test]
    fn a_turn_is_a_run_of_the_same_flow_and_never_the_asking_one() {
        let (ledger, dir) = a_ledger("turns");
        closed_run(&ledger, "cura", "corsa-1", "complete", 20);
        closed_run(&ledger, "cura", "corsa-2", "failed", 30);
        closed_run(&ledger, "altro", "corsa-3", "complete", 40);

        assert_eq!(
            ledger.turns_of_flow("cura", "corsa-2").expect("counting"),
            1,
            "one other run of this flow, whatever became of it"
        );
        assert_eq!(
            ledger.turns_of_flow("cura", "corsa-9").expect("counting"),
            2,
            "a run not yet written down counts none of itself"
        );
        assert_eq!(ledger.turns_of_flow("mai-corsa", "corsa-9").expect("counting"), 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
