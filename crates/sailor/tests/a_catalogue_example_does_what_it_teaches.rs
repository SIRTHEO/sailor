//! An example in the catalogue teaches by what a person sees when it runs, so
//! the result it promises is run here, over the registry the product builds.

use flow::starters;
use flow::subflow::{RunNote, SubflowAction, SubflowHost, SUBFLOW_ACTION};
use flow::system::{self, FlowSource};
use flow::{
    ActionError, ActionRegistry, Clock, ExecutionRequest, Executor, FlowError, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, StepRecord,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// The product's actions over an empty house: no store, no home of this machine.
fn product() -> ActionRegistry {
    registry::registry_in(registry::House::empty(), None, None)
}

/// A call looks for flows in the sources this test declares, not in this
/// process's environment, which every test of the binary shares.
struct DeclaredSources {
    sources: Vec<FlowSource>,
    store: Arc<InMemoryRecordStore>,
}

impl SubflowHost for DeclaredSources {
    fn sources(&self) -> Vec<FlowSource> {
        self.sources.clone()
    }

    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        Ok(Arc::new(product()))
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(self.store.clone())
    }

    fn note_run(&self, _note: &RunNote<'_>) -> Result<(), ActionError> {
        Ok(())
    }
}

/// The flow a person gets from the entry, launched with `text` as the brief when given.
fn made(entry: &str, text: Option<&str>) -> FlowFile {
    let listed = starters::find(entry, &starters::no_extra).expect("the entry is in the catalogue");
    let mut document =
        starters::instantiate(&listed, "made-from-the-catalogue", &BTreeMap::new()).expect("made");
    if let Some(text) = text {
        document["inputs"]["trigger"]["text"] = text.into();
    }
    system::flow_of_document(&document).expect("the engine loads it")
}

fn run(flow: &FlowFile, sources: Vec<FlowSource>) -> Vec<StepRecord> {
    let store = Arc::new(InMemoryRecordStore::default());
    let mut registry = product();
    registry.register(
        SUBFLOW_ACTION,
        SubflowAction::new(Arc::new(DeclaredSources { sources, store: store.clone() })),
    );
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
    let _ = InProcessExecutor.execute(&flow.graph, request, store.as_ref(), &registry, &Tick(AtomicI64::new(0)));
    store.records(&run_id).expect("the records of the run")
}

fn last<'a>(records: &'a [StepRecord], step: &str) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step)
        .max_by_key(|record| (record.attempt, record.epoch))
}

fn answer_of(records: &[StepRecord], step: &str) -> Value {
    let record = last(records, step).unwrap_or_else(|| panic!("«{step}» did not run"));
    assert_eq!(record.outcome, Some(Outcome::Went), "«{step}»: {:?}", record.failure_class);
    record.output.as_ref().and_then(|output| output.get("answer")).cloned().expect("an answer")
}

#[test]
fn an_empty_brief_stops_the_run_at_the_check_and_the_step_after_never_starts() {
    let records = run(&made("a-check-that-stops-the-run", Some("")), vec![FlowSource::builtin()]);

    let check = last(&records, "refuse_an_empty_brief").expect("the check ran");
    assert_ne!(check.outcome, Some(Outcome::Went), "an empty brief passed the check");
    assert!(last(&records, "say_how_long").is_none(), "a step after a failed check started");
}

#[test]
fn a_brief_that_passes_the_check_is_measured_by_the_step_after_it() {
    let records = run(&made("a-check-that-stops-the-run", Some("any words")), vec![FlowSource::builtin()]);

    let answer = answer_of(&records, "say_how_long");
    assert_eq!(answer["said"], "the brief has 9 characters");
}

#[test]
fn with_nothing_replacing_it_the_call_says_the_shipped_flow_ran() {
    let records = run(&made("one-flow-calls-another", None), vec![FlowSource::builtin()]);

    let answer = answer_of(&records, "say_what_came_back");
    assert_eq!(answer["child_status"], "complete");
    assert_eq!(answer["child_origin"], system::BUILTIN_ORIGIN);
    assert_eq!(answer["child_file"], system::PLACE);
}

#[test]
fn a_flow_of_yours_with_that_name_wins_and_the_call_names_its_file() {
    let home: PathBuf = std::env::temp_dir().join(format!("sailor-example-override-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("a scratch home");
    let replacement = home.join("what-this-machine-has.flow.json");
    std::fs::write(
        &replacement,
        r#"{"id": "what-this-machine-has", "description": "a replacement", "graph": {"steps": [
            {"id": "replaced", "deps": [], "action": "shell_check", "max_attempts": 1, "when": null,
             "with": {"command": "printf 'replaced'", "timeout_secs": 10},
             "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}]}, "inputs": {}}"#,
    )
    .expect("the replacement writes");
    let sources = vec![FlowSource::builtin(), FlowSource { origin: system::YOUR_ORIGIN, dir: home.clone() }];

    let records = run(&made("one-flow-calls-another", None), sources);

    let answer = answer_of(&records, "say_what_came_back");
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(answer["child_status"], "complete");
    assert_eq!(answer["child_origin"], system::YOUR_ORIGIN);
    assert_eq!(answer["child_file"], replacement.display().to_string());
}
