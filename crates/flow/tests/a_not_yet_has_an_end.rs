//! A step that keeps answering "not yet" ends when the flow says how long it
//! may: four runs of the emptying flow were asked again every minute for twelve
//! hours about terminals that could never be emptied.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Decision, ExecutionRequest,
    Executor, FlowError, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step,
    ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct Stopped(i64);

impl Clock for Stopped {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0)
    }
}

fn waiting_step(window: Option<u32>) -> Step {
    Step {
        id: "wait".to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: "wait".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: Some(60),
        ask_again_for_secs: window,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        required: false,
        even_after_a_break: false,
        needs: Vec::new(),
        weight: flow::Weight::Light,
    }
}

fn request() -> ExecutionRequest {
    ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    }
}

struct NotYetUntil {
    unlocked: Arc<AtomicBool>,
    asked: Arc<AtomicUsize>,
}

impl Action for NotYetUntil {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if self.unlocked.load(Ordering::SeqCst) {
            Ok(ActionOutcome::Went(json!({"taken": true})))
        } else {
            Ok(ActionOutcome::NotYet("nobody is in there".to_owned()))
        }
    }
}

struct Setup {
    graph: Graph,
    store: InMemoryRecordStore,
    actions: ActionRegistry,
    unlocked: Arc<AtomicBool>,
    asked: Arc<AtomicUsize>,
}

fn setup(window: Option<u32>) -> Setup {
    let unlocked = Arc::new(AtomicBool::new(false));
    let asked = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "wait",
        NotYetUntil {
            unlocked: Arc::clone(&unlocked),
            asked: Arc::clone(&asked),
        },
    );
    Setup {
        graph: Graph::new(vec![waiting_step(window)]).expect("valid graph"),
        store: InMemoryRecordStore::default(),
        actions,
        unlocked,
        asked,
    }
}

fn run_at(setup: &Setup, now: i64) -> Vec<Decision> {
    InProcessExecutor
        .execute(
            &setup.graph,
            request(),
            &setup.store,
            &setup.actions,
            &Stopped(now),
        )
        .expect("the run goes through")
        .decisions
}

#[test]
fn a_step_asked_again_past_its_window_breaks_and_the_run_ends_failed() {
    let scene = setup(Some(3_600));
    run_at(&scene, 1_000);

    let inside = run_at(&scene, 4_000);
    assert!(
        matches!(inside.last(), Some(Decision::NotYet { .. })),
        "inside the window it is still asked again: {inside:?}"
    );

    let past = run_at(&scene, 4_600);
    assert_eq!(
        past.last(),
        Some(&Decision::Failed(vec!["wait".to_owned()])),
        "{past:?}"
    );
    let last = scene
        .store
        .all()
        .into_iter()
        .max_by_key(|record| (record.attempt, record.epoch))
        .expect("the step has records");
    assert_eq!(last.outcome, Some(Outcome::Broke));
    let said = last.said.unwrap_or_default();
    assert!(
        said.contains("3600") && said.contains("nobody is in there"),
        "the record names the window and the last reason: {said}"
    );

    let asked = scene.asked.load(Ordering::SeqCst);
    run_at(&scene, 9_000);
    assert_eq!(
        scene.asked.load(Ordering::SeqCst),
        asked,
        "a run that ended is not asked again"
    );
}

#[test]
fn a_step_with_no_window_is_asked_again_for_ever() {
    let scene = setup(None);
    run_at(&scene, 1_000);
    let later = run_at(&scene, 1_000 + 10 * 365 * 86_400);
    assert!(
        matches!(later.last(), Some(Decision::NotYet { .. })),
        "{later:?}"
    );
}

#[test]
fn an_answer_at_the_end_of_the_window_still_counts() {
    let scene = setup(Some(3_600));
    run_at(&scene, 1_000);
    scene.unlocked.store(true, Ordering::SeqCst);
    let past = run_at(&scene, 4_600);
    assert_eq!(past.last(), Some(&Decision::Complete), "{past:?}");
}
