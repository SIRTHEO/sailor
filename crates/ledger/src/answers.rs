//! The shapes a reader gets back: what a query answers, never what it stores.

use crate::LedgerError;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub failure_class: String,
    pub ended_at: i64,
}

/// What a record answering a repeated call would have saved here. A saving
/// counted on a key that was a pointer, or on a call no engine ever priced, is
/// one nobody can bank: those stay apart so the headline cannot be read alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepeatedCalls {
    pub calls: i64,
    pub served: i64,
    pub served_on_an_unresolved_prompt: i64,
    pub served_micros: i64,
    pub served_without_a_cost: i64,
    /// Calls whose step this store no longer holds: no key, so never served.
    pub calls_without_a_key: i64,
    pub calls_without_a_cost: i64,
    pub spent_micros: i64,
}

/// True when the recorded input still names a value instead of carrying it, so
/// two of them matching says nothing about the two prompts.
pub(crate) fn prompt_is_a_pointer(input: &str) -> bool {
    serde_json::from_str::<Value>(input).is_ok_and(|value| names_a_value(&value))
}

pub(crate) fn names_a_value(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, inner)| {
            key == flow::reference::FROM_KEY
                || key == flow::reference::JOIN_KEY
                || key == flow::reference::JSON_KEY
                || names_a_value(inner)
        }),
        Value::Array(items) => items.iter().any(names_a_value),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesChangedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
}

/// A run with at least one open step, as whoever resumes sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedRun {
    pub run_id: String,
    /// What the run was working on. Empty if nobody ever recorded it.
    pub entity: String,
    pub open_steps: usize,
    pub oldest_started_at: i64,
}

/// A run stopped because it is waiting for somebody.
///
/// **IT CARRIES NO `open_steps`, AND THE ABSENCE IS A STATEMENT.** A waiting
/// run has no open steps: the handed-over one is closed with outcome `Waiting`.
/// A field always reading zero would tell the reader the run had been abandoned
/// halfway, which is the wrong story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingRun {
    pub run_id: String,
    /// Which flow. Empty if nobody ever recorded it.
    pub entity: String,
    /// Since when it has been waiting: the instant the run stopped, or the
    /// instant it started if it has not stopped yet.
    pub waiting_since: i64,
}

/// One table of the projection, and how many rows it holds right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableCount {
    pub name: String,
    pub rows: i64,
}

/// What a browsed statement answered: the columns, the rows as JSON values,
/// and whether the limit cut the answer short.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub truncated: bool,
}

pub(crate) fn browse_with(connection: &Connection, sql: &str, limit: usize) -> Result<Answer, LedgerError> {
    let mut statement = connection.prepare(sql)?;
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let width = columns.len();
    let mut rows = statement.query([])?;
    let mut answer = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows.next()? {
        if answer.len() >= limit {
            truncated = true;
            break;
        }
        let mut cells = Vec::with_capacity(width);
        for index in 0..width {
            let value = match row.get_ref(index)? {
                rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                rusqlite::types::ValueRef::Integer(number) => serde_json::Value::from(number),
                rusqlite::types::ValueRef::Real(number) => serde_json::Value::from(number),
                rusqlite::types::ValueRef::Text(text) => {
                    serde_json::Value::from(String::from_utf8_lossy(text).into_owned())
                }
                rusqlite::types::ValueRef::Blob(bytes) => {
                    serde_json::Value::from(format!("{} bytes", bytes.len()))
                }
            };
            cells.push(value);
        }
        answer.push(cells);
    }
    Ok(Answer {
        columns,
        rows: answer,
        truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedOutputStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub bytes_seen: u64,
    pub bytes_discarded: u64,
}

// ── how it went: the answers a flow can get about its own history ──
//
// **NONE OF THESE STRUCTS CARRIES `input` OR `output`, AND IT IS NOT AN
// OVERSIGHT.** History leaves here for an action any flow can name, and those
// two are the typed data channel: prompts, environments, model replies. Held
// out of the *types*, no field is left for a later slip in `actions` to keep.

/// How many times a step broke, and with what failure class.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepFailureTally {
    /// The denominator: three failures out of three attempts and three out of
    /// two hundred are the same figure and not the same thing.
    pub attempts: i64,
    pub failures: i64,
    /// The runs touched, which are not the failures: a step can break several
    /// times in the same run, one attempt at a time.
    pub runs_affected: i64,
    pub by_class: Vec<FailureClassCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureClassCount {
    /// `None` is a broken step the engine could not classify, and differs from
    /// a class literally named "unknown": here the datum is missing.
    pub failure_class: Option<String>,
    pub failures: i64,
    pub runs_affected: i64,
}

/// A **closed** run, step by step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedRun {
    pub run_id: String,
    pub entity: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub steps: Vec<StepOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub step_id: String,
    pub attempt: u32,
    /// `None` is a step left open inside an already-closed run.
    pub outcome: Option<String>,
    pub failure_class: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub bytes_seen: Option<i64>,
    pub bytes_discarded: Option<i64>,
}

/// Whether a step ever got past its own `when`, across every run its flow has
/// closed. `Skipped` is what the engine writes for a step whose condition never
/// held, before an action is reached; every other outcome means it ran once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepReach {
    /// No closed attempt on record — silence, not evidence.
    NeverClosed,
    /// Every closed attempt was `Skipped`: gated, never once let through.
    AlwaysSkipped,
    ReachedAtLeastOnce,
}

/// How long a step takes, measured on successful attempts only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepDurations {
    /// Whole seconds, already sorted: whoever summarises need not re-sort, and
    /// whoever reads the median need not trust that somebody did.
    pub seconds_sorted: Vec<i64>,
    pub last_seconds: Option<i64>,
    /// Broken attempts, counted but **not** measured: a fast failure would pull
    /// the median down and make a slow step look quick.
    pub failed_samples: i64,
}

/// The raw text of a broken step, as handed to whoever diagnoses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaidExcerpt {
    pub step_id: String,
    pub attempt: u32,
    pub said: String,
    pub truncated: bool,
}
