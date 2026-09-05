//! A flow that keeps failing is a fault nobody wrote down. The ledger counts
//! how many runs in a row each flow lost; this decides which of those streaks
//! deserve the fault-writing flow now. It is one rule for every beat, so the
//! window and the command line cannot come to two answers over one ledger.

use crate::system::FAULT_WRITER;
use std::collections::BTreeSet;

/// How many failed runs in a row make a fault worth writing.
pub const FAILURES_THAT_MAKE_A_FAULT: usize = 3;

/// The most recent closed runs of one flow, as long as every one of them
/// failed: their ids, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureStreak {
    pub flow: String,
    pub runs: Vec<String>,
}

impl FailureStreak {
    pub fn length(&self) -> usize {
        self.runs.len()
    }

    pub fn last_failed_run(&self) -> &str {
        self.runs.first().map(String::as_str).unwrap_or("")
    }
}

/// One fault the beat owes the register: which flow, how long its streak is,
/// and the failed run that stands for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultToWrite {
    pub flow: String,
    pub length: usize,
    pub run_id: String,
}

/// The streaks that deserve a fault now: long enough, never about the fault
/// writer itself, and holding no run a fault was already written about — one
/// streak owes one fault, however long it grows, and a streak that starts
/// afresh after a success owes a new one.
pub fn faults_due(streaks: &[FailureStreak], already_written: &BTreeSet<String>) -> Vec<FaultToWrite> {
    streaks
        .iter()
        .filter(|streak| streak.length() >= FAILURES_THAT_MAKE_A_FAULT)
        .filter(|streak| streak.flow != FAULT_WRITER)
        .filter(|streak| !streak.runs.iter().any(|run| already_written.contains(run)))
        .map(|streak| FaultToWrite {
            flow: streak.flow.clone(),
            length: streak.length(),
            run_id: streak.last_failed_run().to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streak(flow: &str, length: usize) -> FailureStreak {
        streak_from(flow, 1, length)
    }

    fn streak_from(flow: &str, first: usize, last: usize) -> FailureStreak {
        FailureStreak {
            flow: flow.to_owned(),
            runs: (first..=last).rev().map(|n| format!("{flow}-{n}")).collect(),
        }
    }

    #[test]
    fn two_failures_are_not_yet_a_fault_and_three_are() {
        let streaks = vec![streak("notte", 2), streak("relay", 3), streak("sweep", 5)];
        let due = faults_due(&streaks, &BTreeSet::new());
        let flows: Vec<&str> = due.iter().map(|fault| fault.flow.as_str()).collect();
        assert_eq!(flows, vec!["relay", "sweep"], "{due:?}");
        assert_eq!(due[0].run_id, "relay-3");
        assert_eq!(due[0].length, 3);
    }

    #[test]
    fn a_streak_written_about_owes_nothing_more_while_it_grows() {
        let streaks = vec![streak("relay", 3), streak("sweep", 4)];
        let written = BTreeSet::from(["relay-3".to_owned()]);
        let due = faults_due(&streaks, &written);
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!(due[0].flow, "sweep");

        // Two more failures on the same streak: the fault is already written.
        let grown = vec![streak("relay", 5)];
        assert!(faults_due(&grown, &written).is_empty());

        // A success in between, then three fresh failures: a new streak, and
        // a new fault.
        let afresh = vec![streak_from("relay", 7, 9)];
        assert_eq!(faults_due(&afresh, &written).len(), 1);
    }

    #[test]
    fn the_fault_writer_never_writes_about_itself() {
        let streaks = vec![streak(FAULT_WRITER, 9), streak("relay", 3)];
        let due = faults_due(&streaks, &BTreeSet::new());
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!(due[0].flow, "relay");
    }

    #[test]
    fn nothing_failing_owes_nothing() {
        assert!(faults_due(&[], &BTreeSet::new()).is_empty());
    }
}
