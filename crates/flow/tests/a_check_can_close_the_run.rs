//! A run may end on a check instead of on a model saying it is finished.
//!
//! The fifth reason. It sits ahead of the promise because a measured verdict
//! outranks a declared one, it closes the run as *complete* where the other
//! four close it short, and it is the only one that reads a word for equality:
//! absent, failed, timed out and broken all leave the run open.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, ExecutionRequest, Executor,
    Graph, InMemoryRecordStore, InProcessExecutor, RunStops, SharedState, Step, StopReason,
    SystemClock, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Answers the verdict its input names, and declares itself a check.
struct Verdict;

impl Action for Verdict {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        match input.get("say") {
            Some(Value::String(word)) if word == "break" => {
                Err(ActionError::new("check_broke", "the check could not run"))
            }
            Some(said) => Ok(ActionOutcome::Went(said.clone())),
            None => Ok(ActionOutcome::Went(Value::Null)),
        }
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

/// Hands its input back, and is not a check: it stands in for the engine.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

fn step(id: &str, action: &str, deps: &[&str]) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

fn registry() -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("verdict", Verdict);
    actions.register("echo", Echo);
    actions
}

fn request(root_inputs: BTreeMap<String, Value>) -> ExecutionRequest {
    ExecutionRequest {
        run_id: "run".to_owned(),
        root_inputs,
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    }
}

/// A check that says `word`, then an engine step that depends on it.
fn gate_then_engine(word: Value) -> (Graph, ExecutionRequest) {
    let mut gate = step("gate", "verdict", &[]);
    gate.decides_done = true;
    let graph = Graph::new(vec![gate, step("again", "echo", &["gate"])]).expect("a sane graph");
    let roots = [("gate".to_owned(), json!({"say": word}))]
        .into_iter()
        .collect();
    (graph, request(roots))
}

fn run(graph: &Graph, request: ExecutionRequest) -> (Vec<Decision>, Vec<String>, (&'static str, bool)) {
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry(), &SystemClock)
        .expect("the run answers");
    let status = flow::run_status(&execution);
    let opened = store
        .all()
        .into_iter()
        .map(|record| record.step_id)
        .collect();
    (execution.decisions, opened, status)
}

/// The whole point: the check passes, the run is done, and the step that would
/// have called an engine never opens.
#[test]
fn a_passing_check_closes_the_run_without_another_call() {
    let (graph, request) = gate_then_engine(json!({"status": "passed"}));
    let (decisions, opened, status) = run(&graph, request);

    assert_eq!(
        decisions.last(),
        Some(&Decision::Halted {
            reason: StopReason::Checked,
            not_started: vec!["again".to_owned()],
        }),
        "a passed verdict closes the run: {decisions:?}"
    );
    assert_eq!(opened, vec!["gate".to_owned()], "the engine step never opened");
    assert_eq!(
        status,
        ("complete", true),
        "a run a check closed is done, not stopped short"
    );
}

/// **A CHECK THAT COULD NOT RUN IS NOT A PASS.** Five ways of not passing, and
/// none of them ends the run: the follow-up step opens every time.
#[test]
fn a_check_that_did_not_pass_leaves_the_run_open() {
    for word in [
        json!({"status": "failed"}),
        json!({"status": "timed_out"}),
        json!({"status": "PASSED"}),
        json!({"nothing": "said"}),
        json!("break"),
    ] {
        let (graph, request) = gate_then_engine(word.clone());
        let (decisions, opened, _) = run(&graph, request);

        assert!(
            !decisions.iter().any(|decision| matches!(
                decision,
                Decision::Halted {
                    reason: StopReason::Checked,
                    ..
                }
            )),
            "«{word}» is not a pass, yet it closed the run: {decisions:?}"
        );
        assert!(
            word == json!("break") || opened.contains(&"again".to_owned()),
            "«{word}» left the follow-up unopened: {opened:?}"
        );
    }
}

/// A truthy value is not a verdict. `failed` is a string with text in it, so a
/// check read the way `stops_when` reads a promise would close the run on it:
/// the reason `decides_done` is a flag and the word lives in Rust.
#[test]
fn a_failed_verdict_is_truthy_and_still_does_not_close_the_run() {
    let (graph, request) = gate_then_engine(json!({"status": "failed"}));
    let (decisions, opened, status) = run(&graph, request);

    assert!(opened.contains(&"again".to_owned()), "{opened:?}");
    assert_eq!(status.0, "complete", "the run ran to its end: {decisions:?}");
    assert!(
        !decisions.iter().any(|decision| matches!(
            decision,
            Decision::Halted { .. }
        )),
        "nothing halted this run: {decisions:?}"
    );
}

/// A step whose model kept its promise **and** a check that passed: the ledger
/// keeps the measured reason, so a cheap run can be told from a believed one.
#[test]
fn the_ending_says_a_check_closed_it_and_not_a_promise() {
    let mut gate = step("gate", "verdict", &[]);
    gate.decides_done = true;
    let mut claim = step("claim", "echo", &[]);
    claim.stops_when = Some("/done".to_owned());
    let graph = Graph::new(vec![gate, claim, step("again", "echo", &["gate"])])
        .expect("a sane graph");
    let roots = [
        ("gate".to_owned(), json!({"say": {"status": "passed"}})),
        ("claim".to_owned(), json!({"done": true})),
    ]
    .into_iter()
    .collect();
    let (decisions, _, status) = run(&graph, request(roots));

    let Some(Decision::Halted { reason, .. }) = decisions.last() else {
        panic!("the run closed on something: {decisions:?}")
    };
    assert_eq!(*reason, StopReason::Checked, "{decisions:?}");
    assert_eq!(reason.as_text(), "checked");
    assert_eq!(StopReason::from_text("checked"), Some(StopReason::Checked));
    assert_eq!(status, ("complete", true));
}

/// The promise still closes its own run, and still closes it short. Without
/// this the test above would pass on a `Checked` that had swallowed `Promise`.
#[test]
fn a_promise_without_a_check_still_reads_as_a_promise() {
    let mut claim = step("claim", "echo", &[]);
    claim.stops_when = Some("/done".to_owned());
    let graph = Graph::new(vec![claim, step("again", "echo", &["claim"])]).expect("a sane graph");
    let roots = [("claim".to_owned(), json!({"done": true}))]
        .into_iter()
        .collect();
    let (decisions, _, status) = run(&graph, request(roots));

    let Some(Decision::Halted { reason, .. }) = decisions.last() else {
        panic!("the run closed on something: {decisions:?}")
    };
    assert_eq!(*reason, StopReason::Promise);
    assert_eq!(status, ("stopped", false));
}
