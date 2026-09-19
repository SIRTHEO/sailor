//! `cli.step.flow_no_longer_found` used to say a flow was gone and nothing
//! about where it had been sought — a person reading it could not tell a
//! typo'd `SAILOR_FLOWS` from a flow genuinely deleted. It now names every
//! source `flow_of_run` asked.

use ledger::{Ledger, RunRecord};
use sailor::step_cmd::flow_of_run;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sailor-flow-not-found-{name}-{}",
            std::process::id()
        ));
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

/// `SAILOR_FLOWS` is process-global; this file has one test, so nothing races it.
fn with_flows_dir<T>(flows_dir: &std::path::Path, body: impl FnOnce() -> T) -> T {
    let previous = std::env::var("SAILOR_FLOWS").ok();
    std::env::set_var("SAILOR_FLOWS", flows_dir);
    let result = body();
    match previous {
        Some(value) => std::env::set_var("SAILOR_FLOWS", value),
        None => std::env::remove_var("SAILOR_FLOWS"),
    }
    result
}

#[test]
fn the_sentence_names_the_sources_searched() {
    let store = Scratch::new("store");
    let ledger = Ledger::open(&store.0).expect("the store opens");
    ledger
        .record_run(&RunRecord {
            run_id: "run-lost".to_owned(),
            kind: "flow".to_owned(),
            entity: "a-flow-nobody-kept".to_owned(),
            parent_run_id: None,
            started_by: "test".to_owned(),
            status: "waiting".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: None,
            worktree: None,
            stop_reason: None,
        })
        .expect("recording the run");

    // Empty on purpose: the flow this run declares is nowhere under it, which
    // is exactly the case the sentence has to explain.
    let flows = Scratch::new("flows");
    let error = with_flows_dir(&flows.0, || {
        flow_of_run(&ledger, "run-lost").expect_err("no flow answers to that name")
    });

    assert!(error.contains("a-flow-nobody-kept"), "{error}");
    assert!(error.contains("run-lost"), "{error}");
    assert!(
        error.contains(&flows.0.display().to_string()),
        "the declared source's own directory is named: {error}"
    );
}
