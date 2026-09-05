//! The shipped flow that turns a broken run into a line of the fault register.
//! No engine is started and no register is written: every tool resolves to
//! `sh`, and what the engine would answer is printed by the step's arguments.

use actions::ToolResolver;
use flow::{
    ActionRegistry, Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step,
};
use serde_json::{json, Value};

const FLOW_ID: &str = "write-down-what-broke";

fn flow_file() -> FlowFile {
    let text = flow::system::FLOWS
        .iter()
        .find(|(name, _)| *name == FLOW_ID)
        .map(|(_, text)| *text)
        .unwrap_or_else(|| panic!("«{FLOW_ID}» is not among the shipped flows"));
    serde_json::from_str(text).expect("the shipped flow loads")
}

struct EveryToolIsShell;

impl ToolResolver for EveryToolIsShell {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }

    /// The run being written up is this project's own: the step says private,
    /// and an engine with no pact would be refused before spending.
    fn data_pact(&self, _id: &str) -> models::pact::DataPact {
        models::pact::DataPact::DoesNotTrain
    }
}

/// A scratch directory of this test's own, taken down with it.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sailor-written-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

/// The product's registry, over a store and a register of this test's own.
///
/// **NEITHER IS THE MACHINE'S.** `default_registry` would give the fault nodes
/// the register a person actually keeps, and a test that writes there leaves a
/// fault nobody hit.
fn registry(scratch: &Scratch) -> ActionRegistry {
    let ledger = ledger::Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    let mut registry = registry::default_registry(Some(ledger), None);
    actions::faults::register_faults(&mut registry, Some(scratch.0.join(faults::FAULTS_FILE)));
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(EveryToolIsShell),
    );
    registry
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

fn run(graph: &Graph, trigger: Value) -> (Execution, InMemoryRecordStore) {
    let scratch = Scratch::new();
    let store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        run_id: "written".to_owned(),
        root_inputs: [("trigger".to_owned(), trigger)].into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
    };
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry(&scratch), &Tick(0.into()))
        .expect("the execution does not break");
    (execution, store)
}

fn prints(text: &str) -> Value {
    json!(["-c", "cat > /dev/null; printf '%s' \"$1\"", "engine", text])
}

/// A fault the engine would write, with `already_written` as given.
fn written_up(already: bool) -> String {
    json!({
        "already_written": already,
        "happened_on": "03/09",
        "what_happened": "the step that reads the price list read an empty file and called it free",
        "how_it_showed": "a run whose cost came out zero on a paid engine",
        "what_would_prevent": "a test that a missing price list refuses instead of pricing at zero",
        "status": "**aperto**"
    })
    .to_string()
}

/// The graph with the engine's answer replaced, and nothing else touched.
fn graph_answering(already: bool) -> Graph {
    let flow = flow_file();
    let mut steps: Vec<Step> = flow.graph.steps().to_vec();
    for step in &mut steps {
        let Some(with) = step.with.as_mut() else { continue };
        if step.id == "write_it" {
            with["args"] = prints(&written_up(already));
        }
    }
    Graph::new(steps).expect("the graph stays valid")
}

fn trigger_naming(flow: &str) -> Value {
    let mut trigger = flow_file().inputs["trigger"].clone();
    trigger["text"] = json!(flow);
    trigger
}

/// **THE CHECK THAT WOULD HAVE CAUGHT IT IS THE FIELD THAT MATTERS.** The
/// engine is asked for it by name, and the register refuses a fault without it.
#[test]
fn the_flow_asks_for_the_check_that_would_have_caught_it() {
    let flow = flow_file();
    assert_eq!(flow.id, FLOW_ID);
    let ids: Vec<&str> = flow.graph.steps().iter().map(|step| step.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["trigger", "how_it_went", "already_known", "write_it", "record_it"]
    );

    let write = flow.graph.step("write_it").expect("the step exists");
    let with = write.with.as_ref().expect("it carries its values");
    assert_eq!(with["data"], json!("private"), "a run of ours is not public text");
    let shape = &with["answer_shape"]["required"];
    for field in ["already_written", "what_would_prevent", "how_it_showed"] {
        assert!(
            shape.as_array().expect("the required fields").iter().any(|name| name == field),
            "«{field}» is not asked for: {shape}"
        );
    }
    assert_eq!(
        flow.graph.step("record_it").expect("the step exists").action,
        "fault_record"
    );
}

/// The same defect written twice reads as two. The engine says the register
/// already holds this one, and the recording step never opens.
#[test]
fn a_fault_already_in_the_register_is_not_written_a_second_time() {
    let (execution, store) = run(&graph_answering(true), trigger_naming("sweep-the-tree"));

    let records = store.all();
    let recorded = records.iter().find(|record| record.step_id == "record_it");
    assert!(
        recorded.is_none_or(|record| record.outcome == Some(Outcome::Skipped)),
        "the register was written for a fault it already holds: {:?}",
        recorded.map(|record| &record.outcome)
    );
    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "the run closes without writing: {:?}",
        execution.decisions.last()
    );
}

/// The control: with the same graph and the opposite answer the step opens, so
/// the silence above is the condition and not a step nobody could reach.
#[test]
fn a_fault_nobody_has_written_reaches_the_register() {
    let (_, store) = run(&graph_answering(false), trigger_naming("sweep-the-tree"));

    let records = store.all();
    let recorded = records
        .iter()
        .find(|record| record.step_id == "record_it")
        .expect("the recording step was opened");
    assert_ne!(
        recorded.outcome,
        Some(Outcome::Skipped),
        "the condition skipped a fault nobody had written"
    );
    let written = recorded.output.clone().expect("the register answered");
    assert_eq!(
        written["fault"]["what_would_prevent"],
        json!("a test that a missing price list refuses instead of pricing at zero"),
        "what reaches the register is what the engine wrote"
    );
    assert!(written["number"].is_number(), "the register gave it a number: {written}");
}

