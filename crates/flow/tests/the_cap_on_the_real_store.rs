//! The spend cap against the *real* store: executor and `Ledger` together, no
//! stand-in between. `the_spending_cap.rs` builds a `StoreThatCounts` that
//! answers `spent()` from memory, and so measures the executor; `spent_in_run`
//! is tested in `crates/ledger/src/tests.rs`, and so measures the store.
//! Between the two is the joint — the executor querying SQLite mid-run — and an
//! untested joint is where a cap stops stopping without telling anyone.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, ExecutionRequest, Executor, FlowError,
    Graph, InProcessExecutor, Outcome, SharedState, Step, ValueSchema, CURRENT_RUN,
};
use ledger::{Ledger, ModelCallRecord};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// A throwaway directory per test. The counter in the name is not ornament —
/// it is fault 21: `cargo test` sends these through one process and the macOS
/// clock has no nanosecond resolution, so two tests building a name from the
/// pid alone steal each other's directory. One run in twenty failed, on a
/// different test each time.
struct TestDirectory(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-cap-real-store-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the test store's directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An action that really spends: no engine runs here and no money is spent, it
/// writes the row a real engine would (`ModelCallRecord` with a chosen cost).
/// The cost is invented; the road it travels — row written, `SUM` re-read,
/// compared against the cap, front not opened — is the real one. It takes the
/// run from the shared state exactly as `actions::recording_for` does, so if
/// the executor stopped writing `CURRENT_RUN` the cap would see nothing.
struct CostsForReal {
    ledger: Ledger,
    micros: i64,
    /// How many times it ran: the number half this test rests on.
    times: Arc<AtomicUsize>,
}

static NEXT_CALL: AtomicU64 = AtomicU64::new(0);

impl Action for CostsForReal {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.times.fetch_add(1, Ordering::SeqCst);
        let run_id = shared
            .get(CURRENT_RUN)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ActionError::new(
                    "no_run",
                    "the shared state carries no run: no spend could be attributed",
                )
            })?
            .to_owned();
        let sequence = NEXT_CALL.fetch_add(1, Ordering::Relaxed);
        self.ledger
            .record_model_call(&a_call_that_cost(
                &format!("{run_id}:{sequence}"),
                &run_id,
                self.micros,
            ))
            .map_err(|error| ActionError::new("store", error.to_string()))?;
        Ok(ActionOutcome::Went(json!("done")))
    }
}

/// The row a real engine leaves behind, cut down to what the cap reads: the run
/// and the cost.
fn a_call_that_cost(call_id: &str, run_id: &str, micros: i64) -> ModelCallRecord {
    ModelCallRecord {
        call_id: call_id.to_owned(),
        run_id: run_id.to_owned(),
        step_id: None,
        // No session: this test watches the spend cap, and a row that neither
        // opens nor resumes one is the normal case.
        session_id: None,
        session_mode: None,
        work_kind: None,
        fell_back_from: Vec::new(),
        purpose: "external_engine".to_owned(),
        cli: "test-engine".to_owned(),
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: Some(micros),
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: ledger::EngineIdentity::default(),
        retry_chain: vec![],
        error_type: None,
        started_at: 100,
        ended_at: Some(110),
    }
}

/// A clock advancing by one per question.
struct Ticking(AtomicI64);

impl Clock for Ticking {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed))
    }
}

fn step(id: &str, deps: Vec<String>) -> Step {
    Step {
        id: id.to_owned(),
        deps,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        when: None,
        action: "costs".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        with: None,
    }
}

/// Two steps in a row: the second waits for the first, so there are two fronts
/// — and it is between one front and the next that the cap works.
fn two_in_a_row() -> Graph {
    Graph::new(vec![
        step("first", vec![]),
        step("second", vec!["first".to_owned()]),
    ])
    .expect("valid graph")
}

/// How a run on the real store went: the final decision, how many steps ran,
/// and what ends up written in the `steps` table.
struct HowItWent {
    execution: flow::Execution,
    ran: usize,
    written: Vec<String>,
    spent_after: flow::Spend,
}

/// Runs the chain under the given cap, on a `Ledger` opened in a throwaway
/// directory and handed to the executor *as its store*.
fn run_on_a_real_ledger(label: &str, cap: Option<i64>, price_micros: i64) -> HowItWent {
    let directory = TestDirectory::new(label);
    let ledger = Ledger::open(&directory.0).expect("open the store");
    let times = Arc::new(AtomicUsize::new(0));
    let mut actions = flow::ActionRegistry::default();
    actions.register(
        "costs",
        CostsForReal {
            ledger: ledger.clone(),
            micros: price_micros,
            times: Arc::clone(&times),
        },
    );

    let run_id = format!("run-{label}");
    // What the cap really guarantees, written down before anyone counts on
    // more: the check happens *before a front is opened*, never inside a step
    // already running, so the first front of a run is never braked —
    // `how_many_fit` with no observed call stays at its ceiling of four, since
    // narrowing on a number that does not exist would be inventing it. The cap
    // is a brake from the second front onwards.
    let execution = InProcessExecutor
        .execute(
            &two_in_a_row(),
            ExecutionRequest {
                run_id: run_id.clone(),
                root_inputs: Default::default(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: cap,
                stops: flow::RunStops::default(),
            },
            &ledger,
            &actions,
            &Ticking(AtomicI64::new(0)),
        )
        .expect("the execution is not a fault");

    let written = ledger
        .steps(&run_id)
        .expect("re-read the steps table")
        .into_iter()
        .filter(|record| record.outcome == Some(Outcome::Went))
        .map(|record| record.step_id)
        .collect();
    let spent_after = ledger.spent_in_run(&run_id).expect("re-read the spend");

    HowItWent {
        execution,
        ran: times.load(Ordering::SeqCst),
        written,
        spent_after,
    }
}

/// The real spend, read from the real store, stops the run: the first step
/// writes a call of 150 against a cap of 100, and before opening the second
/// front the executor asks the `Ledger`, which counts it with
/// `SUM(cost_micros)` over `model_calls`. The mutant this exists to catch is in
/// `crates/ledger/src/lib.rs` — replace `COALESCE(SUM(cost_micros), 0)` with
/// `0` and every other test stays green while this run reaches the end.
#[test]
fn the_cap_stops_the_run_on_what_the_real_ledger_counted() {
    let went = run_on_a_real_ledger("cap-below-the-cost", Some(100), 150);

    assert_eq!(went.ran, 1, "the first step runs, the second does not");
    let Some(Decision::CapReached(stop)) = went.execution.decisions.last() else {
        panic!(
            "the run should have stopped at the cap, instead: {:?}",
            went.execution.decisions.last()
        );
    };
    assert_eq!(stop.cap_micros, 100);
    assert_eq!(
        stop.spent.micros, 150,
        "the figure comes from the store, not from a counter in the test"
    );
    assert_eq!(stop.spent.calls, 1, "one call recorded, and only one");
    assert!(
        stop.spent.is_complete(),
        "the test engine declares its own cost: nothing is unknown"
    );
    assert_eq!(stop.not_started, vec!["second".to_owned()]);

    // The second step does not exist in the real `steps` table. Not "failed",
    // not "waiting": never opened. That is the only shape in which stopping
    // costs nothing, and what separates a cap from a half-way abort.
    assert_eq!(
        went.written,
        vec!["first".to_owned()],
        "only the step that ran should be in the store"
    );
}

/// The same graph, the same store, with no cap: it reaches the end.
///
/// The half that makes the one above readable: without it, "one step out of
/// two" could be a defect of the executor or the store. The second mutant is
/// putting `spend_cap_micros: None` back into the request — were the test above
/// still green under it, it would be measuring the executor, not the cap.
#[test]
fn without_a_cap_the_same_chain_runs_to_the_end_on_the_same_store() {
    let went = run_on_a_real_ledger("no-cap", None, 150);

    assert_eq!(went.ran, 2, "with no cap both run");
    assert_eq!(went.execution.decisions.last(), Some(&Decision::Complete));
    assert_eq!(
        went.written,
        vec!["first".to_owned(), "second".to_owned()],
        "and both are in the store"
    );
    // Proof the store really recorded two spends: without this line a
    // `record_model_call` failing silently would leave everything green, and
    // the cap above would stop on zero calls for the wrong reason.
    assert_eq!(went.spent_after.micros, 300);
    assert_eq!(went.spent_after.calls, 2);
}

/// A cap of zero does not open even the first front, and the store stays empty.
///
/// `Some(0)` is not `None`: it is someone writing "this flow must not spend
/// anything". The executor's comparison is `>=` on purpose — with `>` the first
/// call would get through, and it was the only one that mattered. Here it shows
/// on the real store: zero rows in `steps`, zero in `model_calls`.
#[test]
fn a_cap_of_zero_writes_nothing_at_all_in_the_real_store() {
    let went = run_on_a_real_ledger("cap-at-zero", Some(0), 150);

    assert_eq!(went.ran, 0, "no step started");
    assert!(matches!(
        went.execution.decisions.last(),
        Some(Decision::CapReached(_))
    ));
    assert!(went.written.is_empty(), "no step in the store");
    assert_eq!(went.spent_after, flow::Spend::default(), "no spend");
}
