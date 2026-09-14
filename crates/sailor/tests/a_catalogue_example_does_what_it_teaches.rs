//! An example in the catalogue teaches by what a person sees when it runs, so
//! the result it promises is run here, over the registry the product builds.

use flow::starters;
use flow::{
    Clock, ExecutionRequest, Executor, FlowError, FlowFile, InMemoryRecordStore, InProcessExecutor,
    Outcome, RecordStore, SharedState, StepRecord,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// The flow a person gets from the entry, launched with `text` as the brief.
fn made(entry: &str, text: &str) -> FlowFile {
    let listed = starters::find(entry).expect("the entry is in the catalogue");
    let mut document =
        starters::instantiate(&listed, "made-from-the-catalogue", &BTreeMap::new()).expect("made");
    document["inputs"]["trigger"]["text"] = text.into();
    flow::system::flow_of_document(&document).expect("the engine loads it")
}

fn run(flow: &FlowFile) -> Vec<StepRecord> {
    let store = InMemoryRecordStore::default();
    let run_id = format!("example-{}", flow.id);
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.clone(),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let registry = registry::registry_in(registry::House::empty(), None, None);
    let _ = InProcessExecutor.execute(&flow.graph, request, &store, &registry, &Tick(AtomicI64::new(0)));
    store.records(&run_id).expect("the records of the run")
}

fn last<'a>(records: &'a [StepRecord], step: &str) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step)
        .max_by_key(|record| (record.attempt, record.epoch))
}

#[test]
fn an_empty_brief_stops_the_run_at_the_check_and_the_step_after_never_starts() {
    let records = run(&made("a-check-that-stops-the-run", ""));

    let check = last(&records, "refuse_an_empty_brief").expect("the check ran");
    assert_ne!(check.outcome, Some(Outcome::Went), "an empty brief passed the check");
    assert!(last(&records, "say_how_long").is_none(), "a step after a failed check started");
}

#[test]
fn a_brief_that_passes_the_check_is_measured_by_the_step_after_it() {
    let records = run(&made("a-check-that-stops-the-run", "any words"));

    let said = last(&records, "say_how_long").expect("the step after the check ran");
    assert_eq!(said.outcome, Some(Outcome::Went), "{:?}", said.failure_class);
    let answer = said.output.as_ref().and_then(|output| output.pointer("/answer/said")).and_then(Value::as_str);
    assert_eq!(answer, Some("the brief has 9 characters"));
}
