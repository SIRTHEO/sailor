//! The steps of a run handed to a person, taken and closed from the window.
//!
//! **THE SAME CODE THE COMMAND LINE RUNS.** `sailor step open` and `close`
//! hold the lock that keeps whoever wrote a dependency from judging it; a
//! second implementation would open that lock in silence the day they drift.

use flow::{Outcome, StepRecord};
use ledger::Ledger;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use ui::gather::default_ledger_dir;

/// A step that waits for a person, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Handed {
    pub step_id: String,
    /// Whom the step was offered to: a label for the reader, not a credential.
    pub holder: String,
    pub mandate: String,
    /// Since when it waits: the instant the step closed as waiting.
    pub since: i64,
    /// The tree the run was born in, so the person can be put to work **in
    /// it** instead of judging from wherever the window happens to stand.
    /// `None` is a real answer: a run started outside every workspace.
    pub worktree: Option<String>,
}

/// The handed steps among a run's records: the latest record of each step,
/// kept only when its outcome is `Waiting`. An older attempt that waited and
/// a newer one that went is a step nobody waits on any more.
pub(crate) fn handed_of(records: &[StepRecord]) -> Vec<Handed> {
    let mut latest: BTreeMap<&str, &StepRecord> = BTreeMap::new();
    for record in records {
        let newer = latest
            .get(record.step_id.as_str())
            .is_none_or(|seen| (record.attempt, record.epoch) >= (seen.attempt, seen.epoch));
        if newer {
            latest.insert(&record.step_id, record);
        }
    }
    latest
        .values()
        .filter(|record| record.outcome == Some(Outcome::Waiting))
        .map(|record| Handed {
            step_id: record.step_id.clone(),
            holder: word_in(&record.input, "holder"),
            mandate: word_in(&record.input, "mandate"),
            since: record.ended_at.unwrap_or(record.started_at),
            worktree: None,
        })
        .collect()
}

fn word_in(input: &Value, key: &str) -> String {
    input.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn open_ledger() -> Result<Ledger, String> {
    let dir = default_ledger_dir();
    Ledger::open(&dir).map_err(|error| format!("cannot open the ledger {}: {error}", dir.display()))
}

#[tauri::command]
pub(crate) fn handed_steps(run_id: String) -> Result<Vec<Handed>, String> {
    let ledger = open_ledger()?;
    let records = ledger
        .steps(&run_id)
        .map_err(|error| format!("cannot read run {run_id}: {error}"))?;
    // **WHERE THE RUN IS, NOT WHERE THE WINDOW IS.** Whoever takes the step
    // opens a terminal on it, and a terminal in the wrong tree is found out at
    // the first command that reads a file.
    let worktree = ledger
        .run_header(&run_id)
        .ok()
        .flatten()
        .and_then(|header| header.worktree);
    Ok(handed_of(&records)
        .into_iter()
        .map(|step| Handed {
            worktree: worktree.clone(),
            ..step
        })
        .collect())
}

/// Takes a handed step as the person at this machine. The engine's answer is
/// the report `sailor step open` prints, refusals included.
#[tauri::command]
pub(crate) fn take_handed_step(run_id: String, step_id: String) -> Result<String, String> {
    let ledger = open_ledger()?;
    let found = BTreeMap::from([
        ("run".to_owned(), run_id),
        ("step".to_owned(), step_id),
        ("as".to_owned(), crate::run::who()),
    ]);
    sailor::step_cmd::open_step_in(&ledger, &found)
}

/// Closes a handed step with the outcome the person declares, then resumes
/// the run through this window: it joins the registry of runs, so the console
/// follows it and Stop applies, and the answer says which root it resumed in.
#[tauri::command]
pub(crate) fn close_handed_step(
    app: tauri::AppHandle,
    runs: tauri::State<'_, std::sync::Arc<crate::run::Runs>>,
    run_id: String,
    step_id: String,
    outcome: String,
    said: Option<String>,
) -> Result<String, String> {
    let ledger = open_ledger()?;
    let flow = sailor::step_cmd::flow_of_run(&ledger, &run_id)?;
    let mut found = BTreeMap::from([
        ("run".to_owned(), run_id.clone()),
        ("step".to_owned(), step_id),
        ("as".to_owned(), crate::run::who()),
        ("outcome".to_owned(), outcome),
    ]);
    if let Some(said) = said.filter(|text| !text.trim().is_empty()) {
        found.insert("said".to_owned(), said);
    }
    let closed = sailor::step_cmd::close_step_in(&ledger, &flow, &found)?;
    let resuming = crate::run::resume(&app, &runs, ledger, flow, run_id)?;
    Ok(format!("{closed}\n{resuming}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(step: &str, attempt: u32, outcome: Option<Outcome>) -> StepRecord {
        let mut made = StepRecord::started(
            "run-1",
            step,
            attempt,
            u64::from(attempt),
            vec![],
            json!({ "holder": "theo", "mandate": "read the diff and say" }),
            vec![],
            100,
        );
        made.outcome = outcome;
        made.ended_at = outcome.map(|_| 140);
        made
    }

    /// **ONLY THE STEPS STILL WAITING, AND ONLY BY THEIR LATEST ATTEMPT.** A
    /// step that waited once and then went is not offered again, and a step
    /// that went is not offered at all; what is offered carries the mandate.
    #[test]
    fn a_handed_step_is_the_latest_attempt_that_still_waits() {
        let records = vec![
            record("build", 1, Some(Outcome::Went)),
            record("review", 1, Some(Outcome::Waiting)),
            record("verdict", 1, Some(Outcome::Waiting)),
            record("verdict", 2, Some(Outcome::Went)),
        ];
        let handed = handed_of(&records);
        assert_eq!(handed.len(), 1, "{handed:?}");
        assert_eq!(handed[0].step_id, "review");
        assert_eq!(handed[0].holder, "theo");
        assert_eq!(handed[0].mandate, "read the diff and say");
        assert_eq!(handed[0].since, 140);
        assert!(handed_of(&[record("build", 1, None)]).is_empty());
    }
}
