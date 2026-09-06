//! What the dashboard could say and the window could not.
//!
//! **WHY THIS MODULE EXISTS.** `sailor ui` has served a page on
//! `127.0.0.1:47831` with five sections for months. The window covers two — the
//! flows, and "what is happening right now". The other three — today's summary,
//! the history of the executions, what is installed — exist only there. While
//! that holds, removing the dashboard is subtracting, not simplifying.
//!
//! The rule is mechanical: before replacing a view, list what it let you do,
//! and check item by item that the new one still lets you.
//!
//! **THE SUMS ARE NOT REDONE HERE**, nor in the canvas. `build_executions` is
//! the same function that serves the dashboard: two sums written in two places
//! would give two figures and nobody would know which to believe. It holds for
//! the inventory too — `default_roots` lives in the crate precisely so that the
//! command line and the page say the same number on the same machine.

use inventory::{collect_survey, default_roots, Inventory};
use serde::Serialize;
use std::collections::BTreeMap;
use ui::dashboard::{build_executions, ExecutionView};
use ui::gather::{default_ledger_dir, gather};

/// The summary of one day.
///
/// **IT CARRIES WHAT IT COULD NOT MEASURE TOO**, and that is the part usually
/// missing. A figure shown with authority and wrong is worse than an absent
/// one. `unmeasured` and `unpriced` say how many calls brought no tokens and no
/// price, so a low figure reads for what it is: low, or incomplete.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct DaySummary {
    /// True if the ledger exists. False is not zero: it is "I do not know".
    pub ledger_present: bool,
    pub runs: usize,
    pub went: usize,
    pub broke: usize,
    pub still_open: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_micros: i64,
    /// Model calls that reported no tokens.
    pub unmeasured: usize,
    /// Model calls that reported no price.
    pub unpriced: usize,
    /// Tokens seen, by model.
    pub tokens_by_model: BTreeMap<String, u64>,
}

/// The history of the executions, most recent first.
///
/// **IT IS NOT "RIGHT NOW" WITH MORE ROWS.** "Right now" asks the ledger what
/// is open and knows nothing of the past; this one carries everything, closed
/// included, and it is the view where a repeating fault is hunted — the same
/// run fallen three times running shows up here and nowhere else.
///
/// A missing ledger gives an empty list, not an error: it is a machine on which
/// nothing has run yet.
#[tauri::command]
pub(crate) fn execution_history() -> Result<Vec<ExecutionView>, String> {
    let ledger_dir = default_ledger_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let data = gather(&ledger_dir)
        .map_err(|error| format!("cannot read the ledger: {error}"))?;
    let Some(data) = data else {
        return Ok(Vec::new());
    };
    let mut executions = build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, now);
    // Most recent on top: this view looks back, and it starts at yesterday.
    executions.reverse();
    Ok(executions)
}

/// The summary of the runs started from a given instant onward.
///
/// **THE CALLER CARRIES THE INSTANT, AND THAT IS NOT LAZINESS.** "Today" is a
/// local calendar day, and the window knows the time zone because its system
/// tells it, while here a whole library would be needed to rediscover it. What
/// stays undelegated is the sum: it lives here, once, because it is the figure
/// a person looks at to decide.
#[tauri::command]
pub(crate) fn day_summary(since: i64) -> Result<DaySummary, String> {
    let ledger_dir = default_ledger_dir();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let data = gather(&ledger_dir)
        .map_err(|error| format!("cannot read the ledger: {error}"))?;
    let Some(data) = data else {
        // Ledger absent: `ledger_present` stays false, and every count is
        // zero. A reader must be able to tell "nothing has run" from "I could
        // not look", and that field is the whole difference.
        return Ok(DaySummary::default());
    };
    let executions = build_executions(&data.runs, &data.steps_by_run, &data.calls_by_run, now);
    let mut summary = DaySummary {
        ledger_present: true,
        ..DaySummary::default()
    };
    for execution in executions.iter().filter(|run| run.started_at >= since) {
        summary.runs += 1;
        let open = !execution.steps_open.is_empty()
            || matches!(execution.status.as_str(), "running" | "open");
        // BROKEN OUTRANKS OPEN. A run with one step fallen and another still
        // in flight is a fault still burning, not work in progress: counting
        // it among the open ones would take it out of the eye of whoever
        // watches the faults.
        let broke = execution.error.is_some()
            || execution.steps_broke > 0
            || matches!(execution.status.as_str(), "failed" | "broke" | "error");
        if broke {
            summary.broke += 1;
        } else if open {
            summary.still_open += 1;
        } else if execution.status == "succeeded" {
            summary.went += 1;
        }
        summary.input_tokens += execution.tokens.input_tokens;
        summary.output_tokens += execution.tokens.output_tokens;
        summary.cached_tokens += execution.tokens.cached_tokens;
        summary.cache_write_tokens += execution.tokens.cache_write_tokens;
        summary.cost_micros += execution.tokens.cost_micros;
        summary.unmeasured += execution.tokens.calls_without_tokens;
        summary.unpriced += execution.tokens.calls_without_cost;
        for (model, tokens) in &execution.tokens_by_model {
            *summary.tokens_by_model.entry(model.clone()).or_insert(0) += tokens.input_tokens
                + tokens.output_tokens
                + tokens.cached_tokens
                + tokens.cache_write_tokens;
        }
    }
    Ok(summary)
}

/// What is installed on this machine: skills, agents, commands, rules, hooks.
///
/// **IT CARRIES WHERE IT LOOKED TOO.** `Inventory` exposes the entries and also
/// the roots actually walked, and that is deliberate: a list silent about where
/// it searched cannot be contradicted. The window shows them, as the dashboard
/// does.
#[tauri::command]
pub(crate) fn machine_inventory() -> Inventory {
    collect_survey(&default_roots(ledger::sailor_home().as_deref()))
}
