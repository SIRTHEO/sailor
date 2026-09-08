//! A step is held by a process, and a pid is not a process: the operating
//! system hands the same number to whoever is born next. These rows carry a
//! live pid on purpose — this one — and differ only in the identity beside it,
//! so nothing here waits for the machine to reuse a number for real.

use flow::{HolderIdentity, StepRecord};
use ledger::{Ledger, StillHeld};
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-holder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn opened(run: &str) -> StepRecord {
    StepRecord::started(run, "step", 1, 1, vec![], serde_json::json!(null), vec![], 1)
}

/// The birth second this machine gives for this process, or the reason it
/// cannot: a test that cannot measure says which of the two it is.
fn born_now() -> Option<i64> {
    ledger::this_process_holds().born_at
}

#[test]
fn a_live_pid_with_another_birth_second_does_not_inherit_the_step() {
    let mut record = opened("run-reused");
    record.held_by_pid = Some(std::process::id());
    let Some(born) = born_now() else {
        assert_eq!(
            ledger::still_held(&record),
            StillHeld::Uncertain,
            "a machine that will not say when a process was born owes a doubt, never a yes"
        );
        return;
    };
    record.held_by = Some(HolderIdentity {
        born_at: Some(born - 1),
        invocation: "an invocation that has ended".to_owned(),
    });
    assert_eq!(
        ledger::still_held(&record),
        StillHeld::Released,
        "the number is alive and it is not the process the row named"
    );
}

/// The control the test above needs: the same live pid, the same birth second,
/// another invocation. If this came out `Released` too, the check above would
/// be measuring nothing.
#[test]
fn the_same_process_under_another_invocation_still_holds_it() {
    let mut record = opened("run-same");
    record.held_by_pid = Some(std::process::id());
    let Some(born) = born_now() else {
        return;
    };
    record.held_by = Some(HolderIdentity {
        born_at: Some(born),
        invocation: "another invocation of this same process".to_owned(),
    });
    assert_eq!(ledger::still_held(&record), StillHeld::Held);
}

#[test]
fn what_this_very_invocation_took_is_held_without_asking_the_machine() {
    let mut record = opened("run-mine");
    record.held_by_pid = Some(std::process::id());
    record.held_by = Some(ledger::this_process_holds());
    assert_eq!(ledger::still_held(&record), StillHeld::Held);
}

/// The row was written on a machine that would not say when a process was
/// born. The number is alive, and that is still not an answer.
#[test]
fn an_identity_with_no_birth_second_is_uncertain_even_on_a_live_pid() {
    let mut record = opened("run-unsaid");
    record.held_by_pid = Some(std::process::id());
    record.held_by = Some(HolderIdentity {
        born_at: None,
        invocation: "an invocation that has ended".to_owned(),
    });
    assert_eq!(ledger::still_held(&record), StillHeld::Uncertain);
}

/// Migration: a row from before the identity existed keeps its number and
/// keeps being read, and what it says is «I do not know».
#[test]
fn a_row_carrying_only_a_number_is_uncertain_and_never_alive() {
    let mut record = opened("run-old");
    record.held_by_pid = Some(std::process::id());
    record.held_by = None;
    assert_eq!(
        ledger::still_held(&record),
        StillHeld::Uncertain,
        "a live number proves nothing about the process that wrote the row"
    );

    let mut dead = opened("run-old-dead");
    dead.held_by_pid = Some(i32::MAX as u32);
    assert_eq!(
        ledger::still_held(&dead),
        StillHeld::Uncertain,
        "and neither does a dead one: the number was somebody else's long ago"
    );
}

#[test]
fn a_pid_that_is_gone_releases_the_step_and_no_pid_holds_nothing() {
    let mut gone = opened("run-gone");
    gone.held_by_pid = Some(i32::MAX as u32);
    gone.held_by = Some(HolderIdentity {
        born_at: Some(1),
        invocation: "an invocation that has ended".to_owned(),
    });
    assert_eq!(ledger::still_held(&gone), StillHeld::Released);

    assert_eq!(
        ledger::still_held(&opened("run-handed")),
        StillHeld::Nobody,
        "a handed step is held by a deadline, not by a process"
    );
}

/// An identity with no number to ask about: the row cannot say who holds it,
/// and the answer is the doubt, not a guess in either direction.
#[test]
fn an_identity_without_a_number_is_refused() {
    let mut record = opened("run-ambiguous");
    record.held_by = Some(ledger::this_process_holds());
    assert_eq!(ledger::still_held(&record), StillHeld::Uncertain);
}

#[test]
fn the_identity_survives_the_store_and_an_old_row_reads_back() {
    let scratch = Scratch::new("round-trip");
    let store = Ledger::open(scratch.0.join("ledger")).expect("open the ledger");

    let mut mine = opened("run-store");
    mine.held_by_pid = Some(std::process::id());
    mine.held_by = Some(ledger::this_process_holds());
    store.append_step_started(&mine).expect("write the step");

    let mut old = opened("run-store-old");
    old.step_id = "step-old".to_owned();
    old.held_by_pid = Some(std::process::id());
    store.append_step_started(&old).expect("write the old shape");

    let held = store.steps("run-store").expect("read the run");
    assert_eq!(held[0].held_by, mine.held_by, "the identity comes back whole");
    assert_eq!(ledger::still_held(&held[0]), StillHeld::Held);

    let read_back = store.steps("run-store-old").expect("read the old run");
    assert_eq!(read_back[0].held_by, None);
    assert_eq!(read_back[0].held_by_pid, Some(std::process::id()));
    assert_eq!(ledger::still_held(&read_back[0]), StillHeld::Uncertain);
}
