//! `StopReason::ByHand` was reachable only through a store written for a test.
//! The trait's `halt_requested` defaults to `false`, and the real ledger took
//! the default: whoever wrote a halt request into it saw the run carry on.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Decision, ExecutionRequest,
    Executor, FlowError, Graph, InProcessExecutor, SharedState, Step, StopReason, ValueSchema,
};
use ledger::Ledger;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-halt-{name}-{}", std::process::id()));
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

struct Stopped(i64);

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0)
    }
}

/// Counts the times it ran: what proves a halt is a step that did **not**.
struct Counts(Arc<AtomicUsize>);

impl Action for Counts {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ActionOutcome::Went(json!({"done": true})))
    }
}

fn graph_of_one_step() -> Graph {
    Graph::new(vec![Step {
        id: "work".to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: "counts".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }])
    .expect("a valid graph")
}

fn request(run_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        holder: None,
        run_id: run_id.to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    }
}

fn ran_and_ended(ledger: &Ledger, run_id: &str) -> (usize, Decision) {
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register("counts", Counts(Arc::clone(&times)));
    let execution = InProcessExecutor
        .execute(
            &graph_of_one_step(),
            request(run_id),
            ledger,
            &actions,
            &Stopped(1_000),
        )
        .expect("the run goes through");
    let last = execution
        .decisions
        .last()
        .cloned()
        .expect("a run reaches a decision");
    (times.load(Ordering::SeqCst), last)
}

/// **THE ABSURD CONTROL, FIRST.** With no request written, the same run has to
/// do its work: a measurement where the step never runs proves nothing about
/// the halt.
#[test]
fn with_nobody_asking_the_run_does_its_work() {
    let scratch = Scratch::new("nobody-asking");
    let ledger = scratch.ledger();
    let (times, ending) = ran_and_ended(&ledger, "run-free");
    assert_eq!(times, 1, "the step had to run: {ending:?}");
    assert!(
        matches!(ending, Decision::Complete),
        "a run nobody stopped completes: {ending:?}"
    );
}

/// The main case: the request is in the store, and the engine reads it through
/// the store it was given.
#[test]
fn a_run_with_a_halt_request_stops_before_it_opens_anything() {
    let scratch = Scratch::new("asked-to-stop");
    let ledger = scratch.ledger();
    ledger
        .request_halt("run-stopped", "the mandate was wrong", "a-reviewer", 900)
        .expect("the request is written");

    let (times, ending) = ran_and_ended(&ledger, "run-stopped");
    assert_eq!(times, 0, "nothing may start once a stop was asked for");
    match ending {
        Decision::Halted {
            reason,
            not_started,
        } => {
            assert_eq!(reason, StopReason::ByHand);
            assert_eq!(not_started, vec!["work".to_owned()]);
        }
        other => panic!("the run has to close by hand: {other:?}"),
    }
}

/// Who asked and why come back out, or the run reads `by_hand` with nothing
/// beside it.
#[test]
fn the_request_says_who_asked_and_why() {
    let scratch = Scratch::new("who-asked");
    let ledger = scratch.ledger();
    ledger
        .request_halt(
            "run-asked",
            "the output names another machine",
            "a-reviewer",
            900,
        )
        .expect("the request is written");
    assert_eq!(
        ledger
            .why_the_halt_was_asked("run-asked")
            .expect("the store answers"),
        Some((
            "a-reviewer".to_owned(),
            "the output names another machine".to_owned()
        ))
    );
    assert_eq!(
        ledger
            .why_the_halt_was_asked("run-nobody-touched")
            .expect("the store answers"),
        None,
        "a run nobody stopped has no reason to give"
    );
}

/// A stop with no reason is refused where it is written, not where it is read.
#[test]
fn a_halt_with_no_reason_is_refused() {
    let scratch = Scratch::new("no-reason");
    let ledger = scratch.ledger();
    ledger
        .request_halt("run-mute", "   ", "a-reviewer", 900)
        .expect_err("a halt with no reason is not written");
    assert_eq!(
        ledger.halt_request("run-mute").expect("the store answers"),
        None
    );
}
