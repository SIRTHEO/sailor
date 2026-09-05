//! A step runs a flow once per element of a list, for real: files on disk, the
//! executor, an in-memory store and the registry all at once, as the sibling
//! test does for `subflow`. What is *not* proved here is declared: the store is
//! in memory, so the ledger's `parent_run_id` column stays `registry`'s to
//! prove.

use flow::for_each::{ForEachAction, FOR_EACH_ACTION};
use flow::subflow::{RunNote, SubflowHost};
use flow::system::FlowSource;
use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Execution, ExecutionRequest,
    Executor, FlowFile, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore,
    SharedState, Step, SystemClock, ValueSchema, AT_ONCE,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ── the test world ──────────────────────────────────────────────────────────

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sailor-for-each-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Self(dir)
    }

    fn put(&self, text: &str) {
        let file: FlowFile = serde_json::from_str(text).expect("the test flow is valid");
        std::fs::write(self.0.join(format!("{}.flow.json", file.id)), text).expect("written");
    }

    fn place(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How long a child waits for its companions before declaring them queued.
/// Generous: the good case never reaches it, the broken case pays it once.
const DEADLINE: Duration = Duration::from_secs(5);

/// The action every child runs. It records what it received, fails on an
/// element that asks it to, and — when told how many companions to expect —
/// does not leave until that many children are alive at once, which is the
/// only way to see concurrency without a stopwatch.
struct Leaf {
    seen: Mutex<Vec<Value>>,
    alive: AtomicUsize,
    most_alive: AtomicUsize,
    expected_together: Option<usize>,
}

impl Leaf {
    fn new(expected_together: Option<usize>) -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
            alive: AtomicUsize::new(0),
            most_alive: AtomicUsize::new(0),
            expected_together,
        }
    }

    fn seen(&self) -> Vec<Value> {
        self.seen.lock().unwrap_or_else(|held| held.into_inner()).clone()
    }
}

impl Action for Leaf {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(input.clone());
        let now_alive = self.alive.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_alive.fetch_max(now_alive, Ordering::SeqCst);
        if let Some(expected) = self.expected_together {
            let until = Instant::now() + DEADLINE;
            while self.most_alive.load(Ordering::SeqCst) < expected {
                if Instant::now() >= until {
                    self.alive.fetch_sub(1, Ordering::SeqCst);
                    return Err(ActionError::new(
                        "on_its_own",
                        format!("waited for {expected} children alive at once and they never were"),
                    ));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // Long enough for the whole group to be alive together before anyone
        // leaves, so the peak below reads the true width and not a race.
        std::thread::sleep(Duration::from_millis(50));
        self.alive.fetch_sub(1, Ordering::SeqCst);
        if input.get("fail").is_some() {
            return Err(ActionError::new("asked_to", "this element asked to fail"));
        }
        Ok(ActionOutcome::Went(json!({ "echo": input.clone() })))
    }

    fn species(&self) -> flow::StepSpecies {
        flow::StepSpecies::Repeatable
    }
}

struct LeafTo(Arc<Leaf>);

impl Action for LeafTo {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0.execute(input, shared)
    }
}

/// One directory of sources, the test's action, a memory store: the registry
/// the children run with is built on first call, as the real host builds it.
struct Bench {
    dir: PathBuf,
    store: Arc<InMemoryRecordStore>,
    leaf: Arc<Leaf>,
    nested: OnceLock<Arc<ActionRegistry>>,
    notes: Mutex<Vec<(String, String, String, String)>>,
}

impl Bench {
    fn new(dir: &Path, leaf: Leaf) -> Arc<Self> {
        Arc::new(Self {
            dir: dir.to_path_buf(),
            store: Arc::new(InMemoryRecordStore::default()),
            leaf: Arc::new(leaf),
            nested: OnceLock::new(),
            notes: Mutex::new(Vec::new()),
        })
    }

    fn registry(self: &Arc<Self>) -> ActionRegistry {
        let mut registry = ActionRegistry::default();
        registry.register(
            FOR_EACH_ACTION,
            ForEachAction::new(Arc::clone(self) as Arc<dyn SubflowHost>),
        );
        registry.register("leaf", LeafTo(Arc::clone(&self.leaf)));
        registry
    }

    /// The headers written: child run, parent run, parent step, status.
    fn notes(&self) -> Vec<(String, String, String, String)> {
        self.notes
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }
}

impl SubflowHost for Bench {
    fn sources(&self) -> Vec<FlowSource> {
        vec![FlowSource {
            origin: "this project",
            dir: self.dir.clone(),
        }]
    }

    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        Ok(self
            .nested
            .get_or_init(|| {
                let mut registry = ActionRegistry::default();
                registry.register("leaf", LeafTo(Arc::clone(&self.leaf)));
                Arc::new(registry)
            })
            .clone())
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::clone(&self.store) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError> {
        self.notes
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push((
                note.run_id.to_owned(),
                note.parent_run_id.to_owned(),
                note.parent_step_id.to_owned(),
                note.status.to_owned(),
            ));
        Ok(())
    }
}

fn step(id: &str, deps: &[&str], action: &str, with: Option<Value>) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
    }
}

/// Runs a graph under the bench and returns the execution.
fn run(bench: &Arc<Bench>, graph: Graph, root_inputs: Value) -> Execution {
    let registry = bench.registry();
    let root_inputs = serde_json::from_value(root_inputs).expect("root inputs are a map");
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-del-padre".to_owned(),
                root_inputs,
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            bench.store.as_ref(),
            &registry,
            &SystemClock,
        )
        .expect("the execution is not an engine fault")
}

/// A one-step graph that runs `foglia` for a literal list.
fn run_over(bench: &Arc<Bench>, items: Value) -> Execution {
    let graph = Graph::new(vec![step(
        "ripeti",
        &[],
        FOR_EACH_ACTION,
        Some(json!({ "flow": "foglia", "items": items })),
    )])
    .expect("valid graph");
    run(bench, graph, json!({}))
}

fn record_of(bench: &Arc<Bench>, step_id: &str) -> flow::StepRecord {
    bench
        .store
        .records("corsa-del-padre")
        .expect("read the steps")
        .into_iter()
        .find(|record| record.step_id == step_id)
        .expect("the step is there")
}

const LEAF: &str = r#"{
  "id": "foglia",
  "description": "an inner flow of a single step",
  "graph": { "steps": [{
    "id": "riporta", "deps": [], "action": "leaf", "max_attempts": 1, "when": null,
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": { "riporta": { "scritto-nel-file": true } }
}"#;

// ── the tests ───────────────────────────────────────────────────────────────

/// The central fact: one child run per element, each receiving its element,
/// and the outputs coming back in the list's order.
#[test]
fn every_element_runs_the_flow_once_and_the_outputs_keep_the_order() {
    let scratch = Scratch::new("order");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(None));

    let execution = run_over(&bench, json!([{"n": 1}, {"n": 2}, {"n": 3}]));

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    let record = record_of(&bench, "ripeti");
    assert_eq!(record.outcome, Some(Outcome::Went));
    let output = record.output.expect("the step has an output");
    let items = output["items"].as_array().expect("a list of outputs");
    assert_eq!(items.len(), 3, "one output per element: {output}");
    for (nth, item) in items.iter().enumerate() {
        assert_eq!(
            item["outputs"]["riporta"]["echo"]["n"],
            json!(nth + 1),
            "element {nth} came back in its own place: {output}"
        );
        assert_eq!(item["flow"], "foglia");
        assert_eq!(item["status"], "complete");
    }
    let runs = output["runs"].as_array().expect("the child runs");
    assert_eq!(runs.len(), 3);
    for run_id in runs {
        let run_id = run_id.as_str().expect("a run id");
        assert!(
            run_id.starts_with("corsa-del-padre::ripeti::"),
            "the child run is traced from its name: {run_id}"
        );
        assert_eq!(
            bench.store.records(run_id).expect("read").len(),
            1,
            "and its steps sit under it, not under the parent"
        );
    }
    let mut seen = bench.leaf.seen();
    seen.sort_by_key(|input| input["n"].as_u64());
    assert_eq!(
        seen,
        vec![json!({"n": 1}), json!({"n": 2}), json!({"n": 3})],
        "each child received its element and nothing of the file's input"
    );
}

/// Every child is a run in the ledger, opened `running` and closed
/// `complete`, tied to the parent run and the step: the traceable half of
/// decision 4, once per element.
#[test]
fn every_child_run_is_recorded_under_the_parent_and_the_step() {
    let scratch = Scratch::new("header");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(None));

    run_over(&bench, json!(["a", "b"]));

    let notes = bench.notes();
    assert_eq!(notes.len(), 4, "two children, each opened and closed: {notes:?}");
    for note in &notes {
        assert_eq!(note.1, "corsa-del-padre");
        assert_eq!(note.2, "ripeti");
    }
    let mut statuses: Vec<&str> = notes.iter().map(|note| note.3.as_str()).collect();
    statuses.sort_unstable();
    assert_eq!(statuses, ["complete", "complete", "running", "running"]);
}

/// An empty list is an empty answer, and nothing opens.
#[test]
fn an_empty_list_comes_back_empty_without_opening_a_child() {
    let scratch = Scratch::new("empty");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(None));

    let execution = run_over(&bench, json!([]));

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    let output = record_of(&bench, "ripeti").output.expect("an output");
    assert_eq!(output["items"], json!([]));
    assert!(bench.notes().is_empty(), "no child run opened: {:?}", bench.notes());
    assert!(bench.leaf.seen().is_empty(), "and no child step ran");
}

/// The list may come from a dependency's output through a pointer: that is
/// how a step becomes as many steps as the previous one produced.
#[test]
fn the_list_can_be_pointed_at_in_a_dependency_output() {
    let scratch = Scratch::new("pointer");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(None));
    let graph = Graph::new(vec![
        step("elenca", &[], "leaf", None),
        step(
            "ripeti",
            &["elenca"],
            FOR_EACH_ACTION,
            Some(json!({ "flow": "foglia", "items": { "$from": "/echo/list" } })),
        ),
    ])
    .expect("valid graph");

    let execution = run(&bench, graph, json!({ "elenca": { "list": ["x", "y"] } }));

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    let output = record_of(&bench, "ripeti").output.expect("an output");
    let items = output["items"].as_array().expect("a list");
    assert_eq!(items.len(), 2, "{output}");
    assert_eq!(items[0]["outputs"]["riporta"]["echo"], "x");
    assert_eq!(items[1]["outputs"]["riporta"]["echo"], "y");
}

/// A failing element fails the step, under a class of its own, and the
/// sentence names the element's index so a person knows which one to look at.
#[test]
fn a_failing_element_fails_the_step_and_is_named_by_its_index() {
    let scratch = Scratch::new("failure");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(None));

    let execution = run_over(&bench, json!([{"n": 0}, {"n": 1, "fail": true}, {"n": 2}]));

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Failed(_))),
        "the parent run stops: {:?}",
        execution.decisions.last()
    );
    let record = record_of(&bench, "ripeti");
    assert_eq!(record.failure_class.as_deref(), Some("for_each_child_failed"));
    let said = record.said.unwrap_or_default();
    assert!(
        said.contains("index 1 of 3"),
        "the sentence names the index of the element: {said}"
    );
    assert!(
        said.contains("subflow_failed"),
        "and carries why the child ended as it did: {said}"
    );
}

/// Children open together, as many as the executor's own front width and no
/// more. Watched by reflection, not by a stopwatch: each child waits to see
/// the others alive, and the peak of children alive at once is the width.
#[test]
fn children_run_together_up_to_the_front_width_and_no_wider() {
    let scratch = Scratch::new("width");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place(), Leaf::new(Some(AT_ONCE)));
    let items: Vec<Value> = (0..AT_ONCE + 2).map(|n| json!({ "n": n })).collect();

    let execution = run_over(&bench, Value::Array(items));

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "every child saw {AT_ONCE} alive at once: {:?}",
        record_of(&bench, "ripeti").said
    );
    assert_eq!(
        bench.leaf.most_alive.load(Ordering::SeqCst),
        AT_ONCE,
        "and never more than the front width"
    );
    assert_eq!(bench.leaf.seen().len(), AT_ONCE + 2, "all of them ran");
}

/// The step names the fields it does not know, which is how `flow check`
/// finds a typo before it costs a paid call.
#[test]
fn a_field_the_step_does_not_know_is_named() {
    let scratch = Scratch::new("fields");
    let bench = Bench::new(scratch.place(), Leaf::new(None));
    let registry = bench.registry();
    let step = registry.get(FOR_EACH_ACTION).expect("registered");

    assert_eq!(
        step.unknown_fields(&json!({ "flow": "foglia", "items": [], "flusso": "x" })),
        vec!["flusso".to_owned()]
    );
    assert!(step
        .unknown_fields(&json!({ "flow": "foglia", "items": [], "inputs": {} }))
        .is_empty());
}
