//! The shipped `price-every-call`: every call of the ledger gets an equivalent
//! cost by the pinned rules, and the run is red while one stays unpriced.

use flow::system::{load_all, FlowSource};
use flow::{
    run_status, Clock, Execution, ExecutionRequest, Executor, FlowError, FlowFile, Graph,
    InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step,
};
use ledger::{EngineIdentity, Ledger, ModelCallRecord};

const FLOW_ID: &str = "price-every-call";

const LIST: &str = r#"{
  "currency": "USD",
  "models": [
    {"id": "model-b", "input_per_million": 2.0, "output_per_million": 20.0}
  ],
  "engines": {
    "engine-a": {"assumed_model": "model-b", "unread_call_equivalent_micros": 7000}
  }
}"#;

fn shipped() -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == FLOW_ID)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .expect("the flow is shipped")
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sailor-price-every-call-flow-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to work in");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

fn call(id: &str, cli: &str) -> ModelCallRecord {
    ModelCallRecord {
        call_id: id.to_owned(),
        run_id: "run-1".to_owned(),
        step_id: Some("ask".to_owned()),
        purpose: "external_engine".to_owned(),
        cli: cli.to_owned(),
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: Some(10),
        output_tokens: Some(10),
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: None,
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: EngineIdentity::default(),
        retry_chain: Vec::new(),
        error_type: None,
        started_at: 100,
        ended_at: Some(160),
        session_id: None,
        work_kind: None,
        fell_back_from: Vec::new(),
        session_mode: None,
        role: None,
        role_resolved_to: Vec::new(),
    }
}

/// The shipped flow over a store of the test's own, with the price step
/// pointed at a list the test wrote: the machine's list must not decide.
fn run(scratch: &Scratch, ledger: Ledger) -> (Execution, InMemoryRecordStore) {
    let list = scratch.0.join("pricing.json");
    std::fs::write(&list, LIST).expect("write the price list");
    let flow = shipped();
    let mut steps: Vec<Step> = flow.graph.steps().to_vec();
    for step in &mut steps {
        if step.id == "price" {
            step.with = Some(serde_json::json!({"price_list": list.display().to_string()}));
        }
    }
    let graph = Graph::new(steps).expect("the graph stays valid");
    let registry = registry::registry_in(registry::House::under(&scratch.0), Some(ledger), None);
    let store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        holder: None,
        run_id: "priced".to_owned(),
        root_inputs: flow.inputs.into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &registry, &Tick(0.into()))
        .expect("the execution does not break");
    (execution, store)
}

#[test]
fn with_every_call_priced_the_run_completes() {
    let scratch = Scratch::new("green");
    let ledger = Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    ledger.record_model_call(&call("a", "engine-a")).unwrap();
    let (execution, _) = run(&scratch, ledger.clone());
    assert_eq!(run_status(&execution), ("complete", true), "{execution:?}");
    assert!(ledger.model_calls_without_cost().unwrap().is_empty());
}

#[test]
fn one_call_deliberately_unpriced_turns_the_run_red_at_the_verdict() {
    let scratch = Scratch::new("red");
    let ledger = Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    ledger.record_model_call(&call("a", "engine-a")).unwrap();
    ledger.record_model_call(&call("stranger", "engine-nobody-priced")).unwrap();
    let (execution, store) = run(&scratch, ledger.clone());
    assert_eq!(run_status(&execution).1, false, "{execution:?}");
    let verdict = store
        .all()
        .into_iter()
        .find(|record| record.step_id == "verdict")
        .expect("the verdict ran");
    assert_eq!(verdict.outcome, Some(Outcome::Broke), "{verdict:?}");
    assert!(
        verdict.said.as_deref().unwrap_or_default().contains("stranger"),
        "the verdict names the call it could not price: {verdict:?}"
    );
    assert_eq!(ledger.model_calls_without_cost().unwrap().len(), 1);
}
