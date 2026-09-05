//! The step the relay used to park on, run by the real engine.
//!
//! The other three nodes write into a live session and need a pseudo-terminal;
//! `take_mandate` reads a file and removes it, so this is the piece provable
//! **without a terminal** — and the piece where the handover stopped. It does
//! not prove the whole relay reaches the end.

use flow::{
    ActionRegistry, Clock, Decision, ExecutionRequest, Executor, FlowError, Graph,
    InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step, ValueSchema,
};
use serde_json::json;
use std::collections::BTreeMap;

struct Stopped(i64);

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0)
    }
}

fn collecting_step(store: &str) -> Step {
    Step {
        id: "raccogli-il-mandato".to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        // The shipped step's own data, plus the store directory a test cannot
        // inherit from the machine running it.
        with: Some(json!({"tty": "ttys004", "not_before": 2_000, "store": store})),
        when: None,
        action: relay::TAKE_MANDATE_ACTION.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

fn request() -> ExecutionRequest {
    ExecutionRequest {
        run_id: "handover".to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    }
}

/// **THE HANDOVER COMPLETES**, and it was fault 62. First run: no mandate, and
/// the step postpones itself instead of parking. The agent writes one. The beat
/// after: the mandate is collected and the run is complete.
#[test]
fn a_mandate_written_after_the_pause_is_collected_by_the_next_beat() {
    let directory =
        terminal::scratch::directory("relay-picked-up").expect("a scratch directory");
    let path = terminal::mandate::address_in(&directory, "ttys004");
    let graph = Graph::new(vec![collecting_step(
        directory.to_str().expect("a readable path"),
    )])
    .expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    relay::register_relay(&mut actions);

    let first = InProcessExecutor
        .execute(&graph, request(), &store, &actions, &Stopped(1_000))
        .expect("the first run goes through");
    assert_eq!(
        first.decisions.last(),
        Some(&Decision::NotYet {
            steps: vec!["raccogli-il-mandato".to_owned()],
            due_at: 1_001,
        }),
        "{:?}",
        first.decisions
    );
    assert_eq!(store.all()[0].outcome, Some(Outcome::NotYet));

    // The agent writes its mandate, after the pause and past the floor.
    terminal::mandate::write(
        &path,
        &terminal::mandate::Mandate {
            text: "where I got to, and what must not be redone".to_owned(),
            at: 3_000,
        },
    )
    .expect("the agent leaves a mandate");

    let second = InProcessExecutor
        .execute(&graph, request(), &store, &actions, &Stopped(1_001))
        .expect("the beat after goes through");
    assert_eq!(
        second.decisions.last(),
        Some(&Decision::Complete),
        "{:?}",
        second.decisions
    );

    let taken = store
        .all()
        .into_iter()
        .find(|record| record.outcome == Some(Outcome::Went))
        .expect("a second attempt, and this one went");
    assert_eq!(
        taken
            .output
            .as_ref()
            .and_then(|value| value["mandate"].as_str()),
        Some("where I got to, and what must not be redone")
    );
    assert!(
        terminal::mandate::read(&path).is_none(),
        "taken means taken away: the next beat would hand the same work on again"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// **AND IT DOES NOT SPIN WAITING.** If a mandate never arrives, the run ends
/// on every beat instead of holding a process still inside the engine.
#[test]
fn a_mandate_that_never_arrives_ends_the_run_instead_of_holding_it() {
    let directory = terminal::scratch::directory("relay-never").expect("a scratch directory");
    let graph = Graph::new(vec![collecting_step(
        directory.to_str().expect("a readable path"),
    )])
    .expect("valid graph");
    let store = InMemoryRecordStore::default();
    let mut actions = ActionRegistry::default();
    relay::register_relay(&mut actions);

    for beat in 0..3 {
        let execution = InProcessExecutor
            .execute(&graph, request(), &store, &actions, &Stopped(1_000 + beat))
            .expect("every beat ends");
        assert!(
            matches!(execution.decisions.last(), Some(Decision::NotYet { .. })),
            "beat {beat}: {:?}",
            execution.decisions
        );
    }
    // Three polls and no failure: `max_attempts: 1` counts breaks, not the
    // times anybody looked.
    assert_eq!(store.all().len(), 3);
    let _ = std::fs::remove_dir_all(&directory);
}
