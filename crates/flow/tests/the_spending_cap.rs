//! The spend cap: when a run stops itself, and how wide it opens. The test
//! store keeps costs because `InMemoryRecordStore` always answers "spent zero",
//! never recording engine calls — the honest answer for it, but a cap test
//! built on it would be green with or without a cap, measuring that nobody
//! spends. What is proved is not that an `if` exists: it is that a capped run
//! and an uncapped one behave *differently* on the same graph, same actions.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Completion, Decision, ExecutionRequest, Executor,
    FlowError, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState,
    Spend, Step, StepRecord, ValueSchema,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A store that, besides the steps, keeps count of what has been spent.
///
/// It wraps the in-memory one instead of rewriting it: the real rules on
/// epochs, duplicate attempts and closes stay in force, and only the missing
/// piece is added here.
struct StoreThatCounts {
    inner: InMemoryRecordStore,
    spent: Mutex<Spend>,
}

impl StoreThatCounts {
    fn new() -> Self {
        Self {
            inner: InMemoryRecordStore::default(),
            spent: Mutex::new(Spend::default()),
        }
    }

    /// Records a call that cost `micros`, as a real engine would.
    fn charge(&self, micros: i64) {
        let mut spent = self.spent.lock().unwrap_or_else(|held| held.into_inner());
        spent.micros += micros;
        spent.calls += 1;
        spent.dearest_micros = Some(spent.dearest_micros.unwrap_or(0).max(micros));
    }

    /// Records a call whose cost is *not known*.
    fn charge_unknown(&self) {
        let mut spent = self.spent.lock().unwrap_or_else(|held| held.into_inner());
        spent.calls += 1;
        spent.calls_without_cost += 1;
    }
}

impl RecordStore for StoreThatCounts {
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
        self.inner.append_started(record)
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError> {
        self.inner
            .close(run_id, step_id, attempt, epoch, completion)
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        self.inner.records(run_id)
    }

    fn spent(&self, _run_id: &str) -> Result<Spend, FlowError> {
        Ok(*self.spent.lock().unwrap_or_else(|held| held.into_inner()))
    }
}

/// An action that costs. Every time it runs it writes its own spend into the
/// store — what a real engine does, and the only way the cap sees it.
struct CostsMoney {
    store: Arc<StoreThatCounts>,
    micros: i64,
    /// How many times it ran: the number half these tests rest on.
    times: Arc<AtomicUsize>,
}

impl Action for CostsMoney {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.times.fetch_add(1, Ordering::SeqCst);
        self.store.charge(self.micros);
        Ok(ActionOutcome::Went(json!("done")))
    }
}

/// An action that spends without knowing how much: the case of codex, which
/// declares the tokens and not the cost.
struct CostsSomethingUnknown {
    store: Arc<StoreThatCounts>,
}

impl Action for CostsSomethingUnknown {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.store.charge_unknown();
        Ok(ActionOutcome::Went(json!("done")))
    }
}

/// A clock advancing by one per question.
struct Ticking(AtomicI64);

impl Clock for Ticking {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

/// A step with no dependencies calling `action`.
fn step(id: &str, action: &str, deps: Vec<String>) -> Step {
    Step {
        id: id.to_owned(),
        deps,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        with: None,
    }
}

/// A chain of two steps: the second waits for the first.
fn two_in_a_row() -> Graph {
    Graph::new(vec![
        step("first", "costs", vec![]),
        step("second", "costs", vec!["first".to_owned()]),
    ])
    .expect("valid graph")
}

/// Runs the chain under the given cap and says how many steps ran.
fn run_with_cap(cap: Option<i64>, price_micros: i64) -> (flow::Execution, usize) {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: price_micros,
            times: Arc::clone(&times),
        },
    );

    let execution = InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: cap,
                stops: flow::RunStops::default(),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("the execution is not a fault");

    (execution, times.load(Ordering::SeqCst))
}

/// The first step spends more than the cap and the second never starts.
///
/// The central fact: whoever stops has not spent in vain — one step done, then
/// a stop before the next, which is the only moment stopping costs nothing.
#[test]
fn a_run_stops_before_the_step_that_would_break_the_cap() {
    let (execution, ran) = run_with_cap(Some(100), 150);

    assert_eq!(ran, 1, "the first step runs, the second does not");
    let Some(Decision::CapReached(stop)) = execution.decisions.last() else {
        panic!(
            "the run should have stopped at the cap, instead: {:?}",
            execution.decisions.last()
        );
    };
    assert_eq!(stop.cap_micros, 100);
    assert_eq!(stop.spent.micros, 150);
    assert_eq!(
        stop.not_started,
        vec!["second".to_owned()],
        "and it says which step is left to do"
    );
}

/// The same graph with no cap reaches the end.
///
/// The half that makes the test above readable: without it, "one step out of
/// two" could be an executor defect rather than the cap doing its work.
#[test]
fn the_same_flow_without_a_cap_runs_to_the_end() {
    let (execution, ran) = run_with_cap(None, 150);

    assert_eq!(ran, 2, "with no cap both run");
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// A cap of zero stops before the first call.
///
/// `Some(0)` is not `None`: it is someone writing "this flow must not spend
/// anything". The comparison is `>=` on purpose — with `>` the first call would
/// get through, and it was the only one that mattered.
#[test]
fn a_cap_of_zero_stops_before_spending_anything() {
    let (execution, ran) = run_with_cap(Some(0), 150);

    assert_eq!(ran, 0, "no step started");
    assert!(matches!(
        execution.decisions.last(),
        Some(Decision::CapReached(_))
    ));
}

/// A wide cap stops nothing: the cap is there and the run reaches the end.
/// Without this, a cap that stopped *always* would be green on all the others.
#[test]
fn a_cap_that_is_never_reached_changes_nothing() {
    let (execution, ran) = run_with_cap(Some(1_000_000), 150);

    assert_eq!(ran, 2);
    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
}

/// A stopped run also says what it does not know.
///
/// An engine that declares no cost leaves a row with no figure: the real spend
/// is higher than the counted one, and a reader must see that written rather
/// than deduce it. Here the first step spends an unknown amount, the second
/// spends past the cap, and the third finds the run closed.
#[test]
fn what_the_cap_does_not_know_is_declared() {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "unknown",
        CostsSomethingUnknown {
            store: Arc::clone(&store),
        },
    );
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: 150,
            times: Arc::clone(&times),
        },
    );
    let graph = Graph::new(vec![
        step("first", "unknown", vec![]),
        step("second", "costs", vec!["first".to_owned()]),
        step("third", "costs", vec!["second".to_owned()]),
    ])
    .expect("valid graph");

    let execution = InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: Some(100),
                stops: flow::RunStops::default(),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("the execution is not a fault");

    let Some(Decision::CapReached(stop)) = execution.decisions.last() else {
        panic!("it should have stopped at the cap");
    };
    assert_eq!(stop.spent.calls, 2, "two calls in all");
    assert_eq!(
        stop.spent.calls_without_cost, 1,
        "one of the two never said what it cost"
    );
    assert!(
        !stop.spent.is_complete(),
        "and the total declares itself incomplete instead of passing for exact"
    );
}

/// The front narrows when the remainder narrows.
///
/// Four independent steps, with cap and prices chosen so two fit in the
/// remainder: they start two at a time instead of four. The number is not a
/// preference — with four calls in flight the worst overshoot is four times the
/// dearest, and none of the four knows about the others.
#[test]
fn the_front_narrows_as_the_money_runs_out() {
    let store = Arc::new(StoreThatCounts::new());
    // One call already made, at 100: the worst-case estimate comes from it.
    store.charge(100);
    let together = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "counts",
        CountsCompany {
            live: Arc::new(AtomicUsize::new(0)),
            most: Arc::clone(&together),
        },
    );
    let graph = Graph::new(
        (1..=4)
            .map(|n| step(&format!("s{n}"), "counts", vec![]))
            .collect(),
    )
    .expect("valid graph");

    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                // Spent 100, cap 350: 250 remain, and two of the dearest seen
                // (100) fit inside that.
                spend_cap_micros: Some(350),
                stops: flow::RunStops::default(),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("the execution is not a fault");

    let seen = together.lock().unwrap_or_else(|held| held.into_inner());
    let most_at_once = seen.iter().copied().max().unwrap_or(0);
    assert_eq!(
        most_at_once, 2,
        "the remainder allowed two at a time, not four: {seen:?}"
    );
}

/// An action that reports how many were alive alongside it when it entered.
struct CountsCompany {
    live: Arc<AtomicUsize>,
    most: Arc<Mutex<Vec<usize>>>,
}

impl Action for CountsCompany {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let now_live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        // Long enough to let the wave's companions enter: without this pause a
        // group of two could file past one at a time and look like one.
        std::thread::sleep(std::time::Duration::from_millis(50));
        self.most
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(now_live.max(self.live.load(Ordering::SeqCst)));
        self.live.fetch_sub(1, Ordering::SeqCst);
        Ok(ActionOutcome::Went(json!("done")))
    }
}

/// The fact holding the tests above together: a step that runs is a step closed
/// as `Went` in the store, not just a counter going up.
#[test]
fn the_step_that_ran_is_closed_in_the_store() {
    let store = Arc::new(StoreThatCounts::new());
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsMoney {
            store: Arc::clone(&store),
            micros: 150,
            times,
        },
    );

    InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: Some(100),
                stops: flow::RunStops::default(),
            },
            store.as_ref(),
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("the execution is not a fault");

    let records = store.records("run").expect("read the steps");
    assert_eq!(records.len(), 1, "the second was never opened");
    assert_eq!(records[0].step_id, "first");
    assert_eq!(records[0].outcome, Some(Outcome::Went));
}
