//! A flow that keeps failing is a fault nobody wrote down. The ledger counts
//! how many runs in a row each flow lost; this decides which of those streaks
//! deserve the fault-writing flow now. It is one rule for every beat, so the
//! window and the command line cannot come to two answers over one ledger.

use crate::system::FAULT_WRITER;
use std::collections::BTreeSet;

/// How many failed runs in a row make a fault worth writing.
pub const FAILURES_THAT_MAKE_A_FAULT: usize = 3;

/// The most recent closed runs of one flow, as long as every one of them
/// failed. `length` counts them; `last_failed_run` is the newest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureStreak {
    pub flow: String,
    pub length: usize,
    pub last_failed_run: String,
}

/// One fault the beat owes the register: which flow, how long its streak is,
/// and the failed run that stands for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultToWrite {
    pub flow: String,
    pub length: usize,
    pub run_id: String,
}

/// The streaks that deserve a fault now: long enough, not written yet, and
/// never about the fault writer itself — a writer that fails must not be
/// asked to write about its own failure for ever.
pub fn faults_due(streaks: &[FailureStreak], already_written: &BTreeSet<String>) -> Vec<FaultToWrite> {
    streaks
        .iter()
        .filter(|streak| streak.length >= FAILURES_THAT_MAKE_A_FAULT)
        .filter(|streak| streak.flow != FAULT_WRITER)
        .filter(|streak| !already_written.contains(&streak.last_failed_run))
        .map(|streak| FaultToWrite {
            flow: streak.flow.clone(),
            length: streak.length,
            run_id: streak.last_failed_run.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streak(flow: &str, length: usize) -> FailureStreak {
        FailureStreak {
            flow: flow.to_owned(),
            length,
            last_failed_run: format!("{flow}-{length}"),
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
    fn a_streak_whose_last_run_was_already_written_is_not_due_again() {
        let streaks = vec![streak("relay", 3), streak("sweep", 4)];
        let written = BTreeSet::from(["relay-3".to_owned()]);
        let due = faults_due(&streaks, &written);
        assert_eq!(due.len(), 1, "{due:?}");
        assert_eq!(due[0].flow, "sweep");

        // A newer failure moves the streak's last run past what was written,
        // and the same flow is due again.
        let grown = vec![FailureStreak {
            flow: "relay".to_owned(),
            length: 4,
            last_failed_run: "relay-4".to_owned(),
        }];
        assert_eq!(faults_due(&grown, &written).len(), 1);
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
