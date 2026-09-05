//! A run's header, written from one place.
//!
//! These twenty lines used to be written twice — in `sailor::flow_cmd` and in
//! the window's shell — and both hard-coded `total_cost_micros: 0` on a field
//! the window displays. Fixing one of the two would have given two different
//! numbers for the same run depending on who launched it.

use std::path::Path;

use flow::{Decision, Execution, FlowFile, SpendStop, StopReason};
use ledger::{Ledger, RunRecord};

/// How a run ended, and whether the process that launched it may exit zero.
///
/// The body lives in `flow::run_status`, next to `Decision` — the type it
/// translates — because the `subflow` step needs it too and cannot see this
/// crate. A new `Decision` forces every copy to be touched, but nothing makes
/// them choose the **same** word, and two words for one state are two histories
/// that cannot be compared.
pub fn execution_status(execution: &Execution) -> (&'static str, bool) {
    flow::run_status(execution)
}

/// The line that tells a person why the run stopped.
///
/// It says what it does not know: the total is the sum of the *known* costs, so
/// whoever is about to raise the cap and relaunch learns beforehand that the
/// real spend is higher.
///
/// And it says what the figure is. "spent 5.00 of a 5.00 cap" reads like a
/// bill that was stopped, and it is not: a local command line is paid by
/// subscription, and what runs out is quota. The figure is what it would have
/// cost through the API — a yardstick, not a charge.
pub fn why_it_stopped(stop: &SpendStop) -> String {
    let unknown = if stop.spent.is_complete() {
        String::new()
    } else {
        format!(
            ", and {} of the {} calls declared no cost — the real spend is higher",
            stop.spent.calls_without_cost, stop.spent.calls
        )
    };
    format!(
        "stopped by the spending cap: {} of a {} cap, as equivalent cost \
         (what it would have cost through the API, not money spent){unknown}. \
         Steps not started: {}",
        in_units(stop.spent.micros),
        in_units(stop.cap_micros),
        if stop.not_started.is_empty() {
            "none".to_owned()
        } else {
            stop.not_started.join(", ")
        }
    )
}

/// Why the run stopped, if it stopped because of the cap.
pub fn stopped_by_cap(execution: &Execution) -> Option<String> {
    match execution.decisions.last() {
        Some(Decision::CapReached(stop)) => Some(why_it_stopped(stop)),
        _ => None,
    }
}

/// Why the run stopped, if it stopped itself: the reason and the steps it did
/// not start.
pub fn halted_by_hand(execution: &Execution) -> Option<String> {
    match execution.decisions.last() {
        Some(Decision::Halted {
            reason,
            not_started,
        }) => Some(why_it_halted(*reason, not_started)),
        _ => None,
    }
}

/// Which of the four reasons closed the run, if one of them did.
pub fn how_it_stopped(execution: &Execution) -> Option<StopReason> {
    match execution.decisions.last() {
        Some(Decision::Halted { reason, .. }) => Some(*reason),
        _ => None,
    }
}

/// The sentence for a run that stopped itself, composed once for every caller.
pub fn why_it_halted(reason: StopReason, not_started: &[String]) -> String {
    format!(
        "{} Steps not started: {}",
        say_the_reason(reason),
        if not_started.is_empty() {
            "none".to_owned()
        } else {
            not_started.join(", ")
        }
    )
}

/// What one of the four reasons says to a person.
pub fn say_the_reason(reason: StopReason) -> String {
    catalogue::say(
        match reason {
            StopReason::Promise => "run.stopped.promise",
            StopReason::Turns => "run.stopped.turns",
            StopReason::ByHand => "run.stopped.by_hand",
            StopReason::Wall => "run.stopped.wall",
        },
        &[],
    )
}

/// Micro-units as a person reads them, to two decimals.
fn in_units(micros: i64) -> String {
    format!("{:.2}", micros as f64 / 1_000_000.0)
}

/// What is known about a run at the moment it is recorded.
///
/// A struct rather than eight positional arguments: `started_at` and `ended_at`
/// sat next to each other, both times, one `i64` and one `Option<i64>`.
/// Swapping them is not something the compiler always catches, and the result
/// is a run that ended before it began.
pub struct FlowRun<'a> {
    pub run_id: &'a str,
    /// `running`, `complete`, `failed`, `waiting`, `stopped`.
    pub status: &'a str,
    pub started_at: i64,
    /// `None` while the run is open.
    pub ended_at: Option<i64>,
    pub error: Option<String>,
    /// Who started it: the window's button, the command line, a schedule. It
    /// tells otherwise identical runs apart.
    pub started_by: &'a str,
    /// Which of the four reasons closed it short, when one of them did. The
    /// status alone says `stopped` for all four, and one word for four endings
    /// cannot be counted.
    pub stop_reason: Option<StopReason>,
}

/// Records — or updates — a run's header.
///
/// **The total is asked for, not declared.** It used to be a hard-coded `0` in
/// both copies, so every run looked free while its own calls carried the right
/// cost one by one. It now comes from `spent_in_run`.
///
/// That total is the sum of the **known** costs: an engine that does not
/// declare its tokens leaves a row without one, and that row stays out. It is
/// an "at least", not an "exactly", and whoever displays it must display how
/// many calls are missing beside it — which is what `Spend::is_complete` is
/// for. The ledger field is a single integer, so there is no better answer
/// here that does not involve inventing a number.
pub fn record_flow_run(ledger: &Ledger, flow: &FlowFile, run: FlowRun<'_>) -> Result<(), String> {
    write_run(ledger, flow, run, None)
}

/// The same row for a **child** run, plus the link to the run that called it.
///
/// A function rather than one more field on `FlowRun`: a new field on a struct
/// built as a literal in two crates would break both for a value only one
/// caller has.
///
/// **The link goes into the ledger, not only into the name.** `parent_run_id`
/// is a column the ledger already had and nobody filled: without it a child run
/// can only be guessed from the prefix of its own id, and guessing is not
/// tracing. The other direction is carried by the parent's step, which keeps
/// the child's `run_id` in its own output.
pub fn record_child_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run: FlowRun<'_>,
    parent_run_id: &str,
) -> Result<(), String> {
    write_run(ledger, flow, run, Some(parent_run_id.to_owned()))
}

/// The workspace a run is born in: the marker found walking up from where the
/// process stands, which is what «where a run happened» means. `None` is an
/// answer and not a gap — outside is a place.
pub fn tree_of(here: &Path) -> Option<String> {
    flow::workspace::find_root(here).map(|root| root.display().to_string())
}

fn where_the_process_stands() -> Option<String> {
    tree_of(&std::env::current_dir().ok()?)
}

fn write_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run: FlowRun<'_>,
    parent_run_id: Option<String>,
) -> Result<(), String> {
    let spent = ledger
        .spent_in_run(run.run_id)
        .map_err(|error| format!("cannot read the spend of run {}: {error}", run.run_id))?;
    let worktree = where_the_process_stands();
    let reason = run.stop_reason;
    let ended_at = run.ended_at;
    ledger
        .record_run(&RunRecord {
            run_id: run.run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.id.clone(),
            parent_run_id,
            started_by: run.started_by.to_owned(),
            status: run.status.to_owned(),
            total_cost_micros: spent.micros,
            error: run.error,
            started_at: run.started_at,
            ended_at,
            worktree: worktree.clone(),
            stop_reason: reason.map(|reason| reason.as_text().to_owned()),
        })
        .map_err(|error| format!("cannot record run {}: {error}", run.run_id))?;
    // The line is left here rather than beside each launcher, so the window and
    // the command line leave the same one. Only a closed run has a line, and
    // only a flow that says it looks after itself.
    if let Some(closed_at) = ended_at.filter(|_| flow.self_care) {
        let said = reason.map(say_the_reason);
        ledger
            .write_self_care_line(
                run.run_id,
                worktree.as_deref().and_then(head_of_the_tree),
                said.as_deref(),
                run.started_by,
                closed_at,
            )
            .map_err(|error| {
                format!("cannot write the self-care line of run {}: {error}", run.run_id)
            })?;
    }
    Ok(())
}

/// The commit a tree stands on, or nothing where there is no tree to ask and no
/// answer to invent.
fn head_of_the_tree(worktree: &str) -> Option<String> {
    let seen = std::process::Command::new("git")
        .args(["-C", worktree, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !seen.status.success() {
        return None;
    }
    let commit = String::from_utf8(seen.stdout).ok()?.trim().to_owned();
    (!commit.is_empty()).then_some(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::Spend;

    fn stopped_at(cap: i64, spent: Spend, not_started: Vec<String>) -> SpendStop {
        SpendStop {
            cap_micros: cap,
            spent,
            not_started,
        }
    }

    /// The figure is an equivalent cost, not a bill, and the sentence says so.
    /// Two places showing the same number with two meanings are two readers
    /// deciding about different things while believing they see the same one.
    #[test]
    fn the_reason_calls_the_figure_an_equivalent_cost_and_not_a_bill() {
        let said = why_it_stopped(&stopped_at(
            5_000_000,
            Spend {
                micros: 5_000_000,
                calls: 2,
                calls_without_cost: 0,
                dearest_micros: Some(3_000_000),
            },
            vec!["verifica".to_owned()],
        ));

        assert!(
            said.contains("equivalent cost"),
            "the sentence must say what kind of figure it is: {said}"
        );
        assert!(said.contains("5.00"), "and carry the number: {said}");
        assert!(
            said.contains("verifica"),
            "and name the step that did not start: {said}"
        );
    }

    /// What the count is missing is said in the same sentence: whoever is about
    /// to raise the cap and relaunch must know beforehand, not after.
    #[test]
    fn what_the_count_is_missing_is_said_in_the_same_sentence() {
        let said = why_it_stopped(&stopped_at(
            100,
            Spend {
                micros: 150,
                calls: 3,
                calls_without_cost: 2,
                dearest_micros: Some(150),
            },
            vec![],
        ));

        assert!(said.contains("2 of the 3 calls"), "{said}");
        assert!(said.contains("real spend is higher"), "{said}");
        assert!(
            said.contains("none"),
            "and an empty front is stated instead of leaving the line cut short: {said}"
        );
    }

    /// The twin: a complete count must carry no warning. Without this, a
    /// warning that is always on would pass the test above — and a warning
    /// that is always on is read by nobody.
    #[test]
    fn a_complete_count_carries_no_warning() {
        let said = why_it_stopped(&stopped_at(
            100,
            Spend {
                micros: 150,
                calls: 1,
                calls_without_cost: 0,
                dearest_micros: Some(150),
            },
            vec![],
        ));

        assert!(!said.contains("real spend is higher"), "{said}");
    }
}
