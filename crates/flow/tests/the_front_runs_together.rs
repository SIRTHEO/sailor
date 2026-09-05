//! The front starts together, and every step knows it is itself. These tests do
//! not time anything: the measure that unmasked fault 7 was a stopwatch — two
//! six-second steps taking twelve — but a stopwatch inside a suite goes red when
//! the machine is busy and green when someone turned everything off. They watch
//! the fact the clock measured only by reflection: the two steps alive in the
//! same instant. Each waits for the other; queued up, the deadline says so.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, Executor, Graph, InMemoryRecordStore,
    InProcessExecutor, Outcome, SharedState, Step, ValueSchema, CURRENT_STEP,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long to wait for the other before declaring it will not come. Generous:
/// the good case never reaches it, and the broken case can afford to be slow
/// once.
const DEADLINE: Duration = Duration::from_secs(5);

/// An action that enters, says it has entered, and does not leave until
/// everyone it waits for has entered too.
struct MeetsTheOthers {
    arrived: Arc<AtomicUsize>,
    expected: usize,
    /// The id each saw as its own, in the order they arrived.
    seen_as: Arc<Mutex<Vec<String>>>,
}

impl Action for MeetsTheOthers {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // Whose step this is, per the shared state it received.
        let mine = shared
            .get(CURRENT_STEP)
            .and_then(Value::as_str)
            .unwrap_or("(nobody)")
            .to_owned();
        self.seen_as
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(mine.clone());

        self.arrived.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + DEADLINE;
        while self.arrived.load(Ordering::SeqCst) < self.expected {
            if Instant::now() >= until {
                return Err(ActionError::new(
                    "on_its_own",
                    format!(
                        "\"{mine}\" waited {} seconds for the other {} steps of the front and \
                         nobody came: the executor is queuing them up",
                        DEADLINE.as_secs(),
                        self.expected - 1
                    ),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(ActionOutcome::Went(json!({ "me": mine })))
    }

    fn species(&self) -> flow::StepSpecies {
        flow::StepSpecies::Repeatable
    }
}

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, flow::FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

fn step(id: &str) -> Step {
    Step {
        id: id.to_owned(),
        deps: vec![],
        action: "meets".to_owned(),
        max_attempts: 1,
        when: None,
        with: None,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

/// Runs `count` independent steps, each waiting for `expected` companions.
/// Returns the ids the steps saw as their own.
fn run_front(count: usize, expected: usize) -> (Vec<Decision>, Vec<String>, Vec<Outcome>) {
    let arrived = Arc::new(AtomicUsize::new(0));
    let seen_as = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "meets",
        MeetsTheOthers {
            arrived: Arc::clone(&arrived),
            expected,
            seen_as: Arc::clone(&seen_as),
        },
    );

    let steps: Vec<Step> = (1..=count).map(|n| step(&format!("step{n}"))).collect();
    let graph = Graph::new(steps).expect("valid graph");
    let store = InMemoryRecordStore::default();
    let request = flow::ExecutionRequest {
        run_id: "run".to_owned(),
        root_inputs: Default::default(),
        gates: vec![],
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };

    let execution = InProcessExecutor
        .execute(&graph, request, &store, &actions, &Tick(AtomicI64::new(0)))
        .expect("the execution reaches the end");

    let outcomes = store
        .all()
        .iter()
        .filter_map(|record| record.outcome)
        .collect();
    let names = seen_as
        .lock()
        .unwrap_or_else(|held| held.into_inner())
        .clone();
    (execution.decisions, names, outcomes)
}

/// Two steps with no dependencies must be alive at the same moment. Put the
/// sequential `for` back in place of the `scope` and this goes red saying
/// "waited 5 seconds for the other steps of the front and nobody came".
#[test]
fn two_independent_steps_are_alive_at_the_same_time() {
    let (_, _, outcomes) = run_front(2, 2);
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes.iter().all(|outcome| *outcome == Outcome::Went),
        "neither should have been left waiting for the other: {outcomes:?}"
    );
}

/// Every step sees its own name, and this is the piece that could go wrong in
/// silence. The current step's id travels in one key of the shared state, and
/// actions read it to attribute the text they produce and *the money they
/// spend*. With two live steps and one map, both would read the same name: one
/// step's costs would land on the other, and nothing would turn red. Each
/// thread gets its own copy — this test is what keeps that true.
#[test]
fn each_step_sees_its_own_identity_not_the_neighbour_one() {
    let (_, mut seen, _) = run_front(3, 3);
    seen.sort();
    assert_eq!(
        seen,
        vec!["step1".to_owned(), "step2".to_owned(), "step3".to_owned()],
        "three steps alive together must see three different names, each its own"
    );
}

/// The ceiling is a declared decision and it shows: with five steps waiting to
/// be five, the fifth cannot enter until someone from the earlier group leaves,
/// and the first four wait in vain. The run still reaches the end with those
/// steps red — a ceiling that blocks must say so, not hang the program.
#[test]
fn the_ceiling_holds_and_the_run_still_ends() {
    let (decisions, _, outcomes) = run_front(5, 5);
    assert_eq!(outcomes.len(), 5, "all five steps were opened");
    assert!(
        outcomes.contains(&Outcome::Broke),
        "whoever waited past the ceiling says so instead of hanging"
    );
    assert!(
        !decisions.is_empty(),
        "and the run produces its decisions all the same"
    );
}
