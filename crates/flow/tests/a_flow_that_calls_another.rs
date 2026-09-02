//! A flow that runs another one, for real. The guard's own functions are tested
//! on their own, and are; but "the `subflow` step runs another flow" involves
//! file loading, source precedence, the executor, the store and the action
//! registry all at once, and a test touching one of them stays green while the
//! whole piece is broken. What this does *not* prove is declared: the store
//! here is in memory, so `parent_run_id` in the real one stays `registry`'s.

use flow::subflow::{RunNote, SubflowAction, SubflowHost, SUBFLOW_ACTION};
use flow::system::FlowSource;
use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Execution, ExecutionRequest,
    Executor, FlowFile, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore,
    SharedState, Step, SystemClock, ValueSchema,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

// ── the test world ──────────────────────────────────────────────────────────

/// A throwaway directory to write the test's `.flow.json` files into.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sailor-subflow-{label}-{}-{:?}",
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

/// An action that records what it received, and returns it.
///
/// It serves two tests at once: that the child runs, and *with what inputs* —
/// the point of the rule that the child sees only what the step declares.
#[derive(Default)]
struct RecordsWhatItGot {
    seen: Mutex<Vec<(Value, SharedState)>>,
}

impl Action for RecordsWhatItGot {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push((input.clone(), shared.clone()));
        Ok(ActionOutcome::Went(json!({ "echo": input.clone() })))
    }
}

/// The test bench: one directory of sources, the test's actions, memory store.
///
/// The registry is built on first call, like the real one. That is the loop the
/// `subflow` step has to close: it runs with the same actions it is registered
/// among. Building it earlier is impossible — it would contain itself — and a
/// different registry would test something else.
struct Bench {
    dir: PathBuf,
    store: Arc<InMemoryRecordStore>,
    watcher: Arc<RecordsWhatItGot>,
    nested: OnceLock<Arc<ActionRegistry>>,
    notes: Mutex<Vec<(String, String, String, String)>>,
}

impl Bench {
    fn new(dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            dir: dir.to_path_buf(),
            store: Arc::new(InMemoryRecordStore::default()),
            watcher: Arc::new(RecordsWhatItGot::default()),
            nested: OnceLock::new(),
            notes: Mutex::new(Vec::new()),
        })
    }

    /// The registry holding the `subflow` step and the test action.
    fn registry(self: &Arc<Self>) -> ActionRegistry {
        let mut registry = ActionRegistry::default();
        registry.register(
            SUBFLOW_ACTION,
            SubflowAction::new(Arc::clone(self) as Arc<dyn SubflowHost>),
        );
        registry.register("echo", EchoTo(Arc::clone(&self.watcher)));
        registry
    }

    /// The headers written: child run, parent, step, status.
    fn notes(&self) -> Vec<(String, String, String, String)> {
        self.notes
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }
}

/// A shell forwarding to the test's single watcher, so `echo` stays one line.
struct EchoTo(Arc<RecordsWhatItGot>);

impl Action for EchoTo {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0.execute(input, shared)
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
        // The loop, closed the way `registry::LedgerHost` closes it.
        Ok(self
            .nested
            .get_or_init(|| {
                let mut registry = ActionRegistry::default();
                registry.register(
                    SUBFLOW_ACTION,
                    SubflowAction::new(Arc::new(BenchAgain(
                        self.dir.clone(),
                        Arc::clone(&self.store),
                        Arc::clone(&self.watcher),
                    ))),
                );
                registry.register("echo", EchoTo(Arc::clone(&self.watcher)));
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

/// The same bench for deeper levels: same directory, same store, same watcher.
/// It exists because every level builds its own registry, exactly as the real
/// one does.
struct BenchAgain(PathBuf, Arc<InMemoryRecordStore>, Arc<RecordsWhatItGot>);

impl SubflowHost for BenchAgain {
    fn sources(&self) -> Vec<FlowSource> {
        vec![FlowSource {
            origin: "this project",
            dir: self.0.clone(),
        }]
    }

    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        let mut registry = ActionRegistry::default();
        registry.register(
            SUBFLOW_ACTION,
            SubflowAction::new(Arc::new(BenchAgain(
                self.0.clone(),
                Arc::clone(&self.1),
                Arc::clone(&self.2),
            ))),
        );
        registry.register("echo", EchoTo(Arc::clone(&self.2)));
        Ok(Arc::new(registry))
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::clone(&self.1) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, _note: &RunNote<'_>) -> Result<(), ActionError> {
        Ok(())
    }
}

/// The parent step that calls `flow`, with no dependencies.
fn calling_step(id: &str, calls: &str, inputs: Value) -> Step {
    Step {
        id: id.to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: Some(json!({ "flow": calls, "inputs": inputs })),
        when: None,
        action: SUBFLOW_ACTION.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
    }
}

/// A key the parent carries in its shared state that the child must never see.
/// Without it the inheritance test would be empty: it would assert the absence
/// of something nobody ever put there.
const PARENT_ONLY: &str = "parents-secret";

/// La radice che il padre ha, e che il figlio deve ereditare. Sta accanto a
/// `PARENT_ONLY` di proposito: le due chiavi provano le due metà della stessa
/// regola — quello che il figlio *riceve* è solo ciò che il passo dichiara,
/// ma *dove lavora* non è un ingresso e scende comunque.
const A_ROOT: &str = "/una/radice";

/// Runs a one-step graph that calls `calls`.
fn run_calling(bench: &Arc<Bench>, calls: &str, inputs: Value, cap: Option<i64>) -> Execution {
    let graph = Graph::new(vec![calling_step("chiamata", calls, inputs)]).expect("valid graph");
    let registry = bench.registry();
    let mut shared = SharedState::new();
    shared.insert(PARENT_ONLY.to_owned(), json!("must not reach the child"));
    shared.insert(flow::WORKSPACE_ROOT.to_owned(), json!(A_ROOT));
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-del-padre".to_owned(),
                root_inputs: Default::default(),
                gates: Vec::new(),
                shared,
                spend_cap_micros: cap,
            },
            bench.store.as_ref(),
            &registry,
            &SystemClock,
        )
        .expect("the execution is not an engine fault")
}

/// The parent step as the store closed it.
fn parent_step(bench: &Arc<Bench>) -> flow::StepRecord {
    bench
        .store
        .records("corsa-del-padre")
        .expect("read the steps")
        .into_iter()
        .find(|record| record.step_id == "chiamata")
        .expect("the step is there")
}

// ── the test flows ──────────────────────────────────────────────────────────

const LEAF: &str = r#"{
  "id": "foglia",
  "description": "an inner flow of a single step",
  "graph": { "steps": [{
    "id": "riporta", "deps": [], "action": "echo", "max_attempts": 1, "when": null,
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": { "riporta": { "scritto-nel-file": true } }
}"#;

const HERE: &str = r#"{
  "id": "andata",
  "description": "calls ritorno",
  "graph": { "steps": [{
    "id": "vai", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
    "with": { "flow": "ritorno" },
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": {}
}"#;

const BACK: &str = r#"{
  "id": "ritorno",
  "description": "calls andata back: that is the loop",
  "graph": { "steps": [{
    "id": "torna", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
    "with": { "flow": "andata" },
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": {}
}"#;

// ── the tests ───────────────────────────────────────────────────────────────

/// The central fact: a step runs another flow.
///
/// The child runs, its output comes back inside the step's output, and the
/// parent step carries the child's `run_id` — which is the traceable half of
/// decision 4.
#[test]
fn a_step_runs_another_flow_and_carries_back_its_output() {
    let scratch = Scratch::new("runs");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    let execution = run_calling(&bench, "foglia", json!({}), None);

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    let record = parent_step(&bench);
    assert_eq!(record.outcome, Some(Outcome::Went));
    let output = record.output.expect("the step has an output");
    assert_eq!(output["flow"], "foglia");
    assert_eq!(output["origin"], "this project");
    assert_eq!(output["status"], "complete");
    assert_eq!(
        output["outputs"]["riporta"]["echo"]["scritto-nel-file"],
        json!(true),
        "the child's terminal step output is the step's output: {output}"
    );

    let child = output["run_id"].as_str().expect("the child has a run");
    assert!(
        child.starts_with("corsa-del-padre::chiamata::"),
        "the child run is traced from its name: {child}"
    );
    assert_eq!(
        bench.store.records(child).expect("read").len(),
        1,
        "and its steps sit under it, not under the parent"
    );
}

/// The child run is a run, with the parent written beside it.
///
/// Opened `running` and closed `complete`, with the step that called it.
/// Without this, "traceable" would be a word in a comment.
#[test]
fn the_child_run_names_the_run_and_the_step_that_called_it() {
    let scratch = Scratch::new("header");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "foglia", json!({}), None);

    let notes = bench.notes();
    assert_eq!(notes.len(), 2, "one at open and one at close: {notes:?}");
    assert_eq!(notes[0].1, "corsa-del-padre");
    assert_eq!(notes[0].2, "chiamata");
    assert_eq!(notes[0].3, "running");
    assert_eq!(notes[1].3, "complete");
    assert_eq!(notes[0].0, notes[1].0, "the same run, opened and closed");
}

/// The child sees what the step declares, and not the parent's state.
///
/// The step's inputs win over those written in the child's own file, and the
/// parent's shared map does not arrive: if it did, nobody could say any more
/// where a value came from.
#[test]
fn the_child_gets_the_declared_inputs_and_not_the_parent_state() {
    let scratch = Scratch::new("inputs");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(
        &bench,
        "foglia",
        json!({ "riporta": { "dal-passo": "questo" } }),
        None,
    );

    let seen = bench
        .watcher
        .seen
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    let (input, shared) = seen.first().expect("the child ran");
    assert_eq!(input["dal-passo"], "questo", "the step's input wins");
    assert!(
        input.get("scritto-nel-file").is_none(),
        "and replaces the file's for that key: {input}"
    );
    assert_eq!(
        shared
            .get(flow::CURRENT_RUN)
            .and_then(Value::as_str)
            .map(|run| run.contains("corsa-del-padre")),
        Some(true),
        "the child's run carries the parent in its name"
    );
    assert!(
        !shared.contains_key(PARENT_ONLY),
        "nothing of the parent's shared state reaches the child: {shared:?}"
    );
    // L'eccezione è una sola e ha un nome: la radice del progetto, che non è
    // stato del padre ma la stessa macchina sotto tutti e due. La prova che la
    // difende sta qui sotto.
}

/// **DOVE LAVORA IL FIGLIO NON È UN INGRESSO: È LA RADICE DEL PADRE.**
///
/// Senza questa riga in `subflow.rs` il figlio non riceve `workspace.root`, e
/// ogni suo passo cade sulla cartella del processo: `shell_check` applica
/// `current_dir` solo se `workdir` è `Some`, e nessuno gliela offre. Non
/// fallisce — lavora nel posto sbagliato, che è il guasto 25 preso dalla porta
/// di servizio, e proprio la forma che rende il riuso per chiamata inservibile:
/// un flusso «accendi la macchina» chiamato da un altro accenderebbe la
/// macchina di qualunque cartella si trovi a essere il `cwd`.
#[test]
fn the_child_works_where_the_parent_works() {
    let scratch = Scratch::new("radice");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "foglia", json!({}), None);

    let seen = bench
        .watcher
        .seen
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    let (_, shared) = seen.first().expect("the child ran");
    assert_eq!(
        shared.get(flow::WORKSPACE_ROOT).and_then(Value::as_str),
        Some(A_ROOT),
        "the root reaches the child without the child asking: {shared:?}"
    );
}

/// **E UN PADRE SENZA RADICE NON NE INVENTA UNA.** Assente resta assente: il
/// figlio fallirà dicendolo, come lo direbbe il padre. Un ripiego qui sarebbe
/// il guasto 25 scritto due volte.
#[test]
fn a_parent_without_a_root_hands_the_child_none() {
    let scratch = Scratch::new("senza-radice");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    let graph = Graph::new(vec![calling_step("chiamata", "foglia", json!({}))]).expect("valid graph");
    let registry = bench.registry();
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-senza-radice".to_owned(),
                root_inputs: Default::default(),
                gates: Vec::new(),
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            bench.store.as_ref(),
            &registry,
            &SystemClock,
        )
        .expect("the execution is not an engine fault");

    let seen = bench
        .watcher
        .seen
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    let (_, shared) = seen.first().expect("the child ran");
    assert!(
        !shared.contains_key(flow::WORKSPACE_ROOT),
        "no root invented for the child: {shared:?}"
    );
}

/// Two flows that call each other stop, and the error names the chain.
///
/// This is the case the graph's own cycle check *cannot* see: each file alone
/// is a perfectly acyclic graph, and the loop exists only between them. It must
/// say who calls whom — "cycle detected" cannot be repaired, since the reader
/// has to remove the edge and to remove it they must see it.
#[test]
fn two_flows_that_call_each_other_stop_with_the_chain_written_out() {
    let scratch = Scratch::new("loop");
    scratch.put(HERE);
    scratch.put(BACK);
    let bench = Bench::new(scratch.place());

    let execution = run_calling(&bench, "andata", json!({}), None);

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Failed(_))),
        "the parent run stops: {:?}",
        execution.decisions.last()
    );
    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("subflow_cycle"));
    let said = record.said.unwrap_or_default();
    assert!(
        said.contains("andata → ritorno → andata"),
        "the error must name the chain, not just say \"cycle\": {said}"
    );
}

/// A flow that calls itself is the same fault, shorter.
#[test]
fn a_flow_that_calls_itself_names_itself_twice() {
    let scratch = Scratch::new("loner");
    scratch.put(
        r#"{
      "id": "solitario",
      "description": "calls itself",
      "graph": { "steps": [{
        "id": "ancora", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
        "with": { "flow": "solitario" },
        "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
      }] },
      "inputs": {}
    }"#,
    );
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "solitario", json!({}), None);

    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("subflow_cycle"));
    assert!(
        record
            .said
            .unwrap_or_default()
            .contains("solitario → solitario"),
        "the shortest loop is named like the others"
    );
}

/// A name no source knows is not a loop.
///
/// Without this, a guard that said "cycle" to every call would stay green on
/// the tests above. And the error must say *where it looked*: a missing flow is
/// repaired by writing it in the right place.
#[test]
fn a_call_to_a_flow_that_does_not_exist_says_where_it_looked() {
    let scratch = Scratch::new("absent");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "mai-scritto", json!({}), None);

    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("unknown_subflow"));
    let said = record.said.unwrap_or_default();
    assert!(said.contains("mai-scritto"), "it says which flow: {said}");
    assert!(said.contains("this project"), "and where it looked: {said}");
}

/// The parent's cap holds for the child too.
///
/// The child declares no cap: were it to inherit "no limit", moving the spend
/// into a subflow would annul anyone's cap. Here the parent has zero to spend
/// and the child stops before its first step — it receives the remainder, not
/// the absence.
#[test]
fn the_child_inherits_what_is_left_of_the_parent_cap() {
    let scratch = Scratch::new("cap");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "foglia", json!({}), Some(0));

    // With a cap of zero the parent does not open even its own step: the cap
    // works at the level above. The real proof is the lines below.
    let with_room = Bench::new(scratch.place());
    run_calling(&with_room, "foglia", json!({}), Some(1_000_000));
    let record = parent_step(&with_room);
    assert_eq!(
        record.outcome,
        Some(Outcome::Went),
        "with room to spare the child runs: {record:?}"
    );

    let seen = with_room
        .watcher
        .seen
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    let (_, shared) = seen.first().expect("the child ran");
    assert_eq!(
        shared.get(flow::CURRENT_CAP).and_then(Value::as_i64),
        Some(1_000_000),
        "and runs under the cap left to it by the parent, not uncapped: {shared:?}"
    );
}

/// The step names the fields it does not know, which is how `flow check` finds
/// a typo before it costs a paid call.
#[test]
fn a_field_the_step_does_not_know_is_named() {
    let scratch = Scratch::new("fields");
    let bench = Bench::new(scratch.place());
    let registry = bench.registry();
    let step = registry.get(SUBFLOW_ACTION).expect("registered");

    assert_eq!(
        step.unknown_fields(&json!({ "flow": "foglia", "inputs": {}, "flusso": "foglia" })),
        vec!["flusso".to_owned()]
    );
    assert!(step
        .unknown_fields(&json!({ "flow": "foglia", "inputs": {} }))
        .is_empty());
}
