//! Counting runs per flow used to mean loading the whole projection, which is
//! hundreds of megabytes on a real machine — so nobody asked, and eleven
//! shipped flows had never run without anybody noticing. Goal #47.

use ledger::{Ledger, RunRecord};
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-runs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }

    fn ledger(&self) -> Ledger {
        Ledger::open(&self.0).expect("a store on disk")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(run_id: &str, entity: &str, at: i64) -> RunRecord {
    RunRecord {
        run_id: run_id.to_owned(),
        kind: "flow".to_owned(),
        entity: entity.to_owned(),
        parent_run_id: None,
        started_by: "a test".to_owned(),
        status: "complete".to_owned(),
        total_cost_micros: 0,
        error: None,
        started_at: at,
        ended_at: Some(at),
        worktree: None,
        stop_reason: None,
    }
}

#[test]
fn a_flow_with_runs_carries_its_count_and_its_last_start() {
    let scratch = Scratch::new("counted");
    let ledger = scratch.ledger();
    for (id, at) in [("one", 100), ("two", 300), ("three", 200)] {
        ledger.record_run(&run(id, "keep-the-index-fresh", at)).expect("a run is written");
    }
    ledger.record_run(&run("other", "lab-status", 50)).expect("a second flow");

    let counted = ledger.runs_by_entity().expect("the counts read back");

    let fresh = counted.iter().find(|(entity, ..)| entity == "keep-the-index-fresh");
    assert_eq!(fresh, Some(&("keep-the-index-fresh".to_owned(), 3, 300)));
    let lab = counted.iter().find(|(entity, ..)| entity == "lab-status");
    assert_eq!(lab, Some(&("lab-status".to_owned(), 1, 50)));
}

#[test]
fn a_flow_that_never_ran_is_absent_rather_than_zero() {
    let scratch = Scratch::new("absent");
    let ledger = scratch.ledger();
    ledger.record_run(&run("one", "lab-status", 100)).expect("a run is written");

    let counted = ledger.runs_by_entity().expect("the counts read back");

    assert!(
        !counted.iter().any(|(entity, ..)| entity == "cut-a-release"),
        "a flow with no run of its own has no row, and the caller reads that as never run",
    );
}

#[test]
fn an_empty_store_answers_nothing_rather_than_failing() {
    let scratch = Scratch::new("empty");
    let counted = scratch.ledger().runs_by_entity().expect("an empty store still answers");
    assert!(counted.is_empty());
}
