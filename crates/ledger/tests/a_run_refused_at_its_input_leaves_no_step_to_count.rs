//! A run refused at its own input writes no step, so every step-shaped reading
//! calls it silence. One flow failed this way every thirty minutes for a day
//! and a half with nothing saying so.

use flow::StepRecord;
use ledger::{Ledger, RunRecord};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("sailor-stillborn-{}-{serial}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the test directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn failed(run_id: &str, entity: &str, at: i64, error: &str) -> RunRecord {
    RunRecord {
        run_id: run_id.to_owned(),
        kind: "flow".to_owned(),
        entity: entity.to_owned(),
        parent_run_id: None,
        started_by: "a test".to_owned(),
        status: "failed".to_owned(),
        total_cost_micros: 0,
        error: Some(error.to_owned()),
        started_at: at,
        ended_at: Some(at),
        worktree: None,
        stop_reason: None,
    }
}

#[test]
fn a_flow_whose_runs_never_reached_a_step_is_counted_and_one_that_did_is_not() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    for n in 0..3 {
        ledger
            .record_run(&failed(
                &format!("never-{n}"),
                "keep-the-index-fresh",
                100 + n,
                "$.text: expected required property",
            ))
            .expect("write the run");
    }
    ledger
        .record_run(&failed("began", "another-flow", 200, "it broke later"))
        .expect("write the run");
    ledger
        .append_step_started(&StepRecord::started(
            "began",
            "a-step",
            1,
            1,
            vec![],
            serde_json::json!(null),
            vec![],
            200,
        ))
        .expect("write the step");

    let found = ledger.runs_that_died_before_a_step(3).expect("read");
    assert_eq!(found.len(), 1, "a run that reached a step was counted: {found:?}");
    assert_eq!(found[0].flow, "keep-the-index-fresh");
    assert_eq!(found[0].times, 3);
    assert_eq!(found[0].last_at, 102);
    assert_eq!(
        found[0].error.as_deref(),
        Some("$.text: expected required property")
    );
}

#[test]
fn a_flow_under_the_threshold_is_not_named() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    ledger
        .record_run(&failed("once", "a-flow", 100, "refused"))
        .expect("write the run");
    assert!(ledger
        .runs_that_died_before_a_step(3)
        .expect("read")
        .is_empty());
    assert_eq!(ledger.runs_that_died_before_a_step(1).expect("read").len(), 1);
}
