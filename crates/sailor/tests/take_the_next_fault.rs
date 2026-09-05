//! The shipped `take-the-next-fault`: the oldest open fault becomes the
//! engine's whole mandate, and with nothing open the engine is never started.
//! No engine runs here: every tool resolves to `sh`, which prints what the
//! engine would answer, and the register is a scratch one of the test's own.

use actions::ToolResolver;
use flow::system::{load_all, FlowSource};
use flow::{
    ActionRegistry, Clock, Condition, Decision, Execution, ExecutionRequest, Executor, FlowError,
    FlowFile, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, SharedState, Step,
};
use serde_json::{json, Value};

const FLOW_ID: &str = "take-the-next-fault";

fn shipped() -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == FLOW_ID)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .expect("the flow is shipped")
}

fn repair_stdin(flow: &FlowFile) -> String {
    let repair = flow.graph.step("repair").expect("the engine step");
    serde_json::to_string(&repair.with.as_ref().expect("with")["stdin"]).expect("json")
}

struct EveryToolIsShell;

impl ToolResolver for EveryToolIsShell {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }

    /// The code being repaired is this project's own: the step says private.
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
            "sailor-next-fault-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to work in");
        Scratch(path)
    }

    fn register(&self) -> std::path::PathBuf {
        self.0.join(faults::FAULTS_FILE)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The product's registry over a register of this test's own: the machine's
/// register may hold an open fault, and the engine step would then start.
fn registry(scratch: &Scratch) -> ActionRegistry {
    let ledger = ledger::Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    let mut registry = registry::default_registry(Some(ledger), None);
    actions::faults::register_faults(&mut registry, Some(scratch.register()));
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

fn run(scratch: &Scratch, graph: &Graph) -> (Execution, InMemoryRecordStore) {
    let mut store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        run_id: "taken".to_owned(),
        root_inputs: shipped().inputs.into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
    };
    let execution = InProcessExecutor
        .execute(graph, request, &mut store, &registry(scratch), &mut Tick(0.into()))
        .expect("the execution does not break");
    (execution, store)
}

/// What the engine would answer, printed by the shell in its place.
fn answering() -> String {
    json!({
        "reproduced": true,
        "fixed": true,
        "test": "a_missing_price_list_refuses_instead_of_pricing_at_zero, in the pricing crate",
        "changed": "the reader of the price list refuses an empty file",
        "left_open": ""
    })
    .to_string()
}

/// The graph with the engine's answer replaced, and nothing else touched.
fn graph_answering() -> Graph {
    let mut steps: Vec<Step> = shipped().graph.steps().to_vec();
    for step in &mut steps {
        if step.id == "repair" {
            let with = step.with.as_mut().expect("the engine step carries its values");
            with["args"] = json!(["-c", "cat > /dev/null; printf '%s' \"$1\"", "engine", answering()]);
        }
    }
    Graph::new(steps).expect("the graph stays valid")
}

fn an_open_fault(scratch: &Scratch) -> faults::Fault {
    faults::Faults::open(scratch.register())
        .expect("a register of our own")
        .record(&faults::Draft {
            happened_on: "03/09".to_owned(),
            what_happened: "the step that reads the price list read an empty file and called it free"
                .to_owned(),
            how_it_showed: "a run whose cost came out zero on a paid engine".to_owned(),
            what_would_prevent: "a test that a missing price list refuses instead of pricing at zero"
                .to_owned(),
            status: "**aperto**".to_owned(),
        })
        .expect("the fault is recorded")
}

/// **THE READING COMES FIRST, AND THE ENGINE WAITS ON IT ALONE**: one
/// dependency, so its pointers are bare, and the condition reads `/open`.
#[test]
fn the_fault_is_read_then_handed_to_the_engine_only_when_one_is_open() {
    let flow = shipped();
    let steps: Vec<(&str, &str)> = flow
        .graph
        .steps()
        .iter()
        .map(|step| (step.id.as_str(), step.action.as_str()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("trigger", "trigger"),
            ("next", "fault_next"),
            ("repair", "external_engine"),
        ]
    );
    let repair = flow.graph.step("repair").expect("the engine step");
    assert_eq!(repair.deps, vec!["next"]);
    assert_eq!(
        repair.when,
        Some(Condition::PointerEquals {
            pointer: "/open".to_owned(),
            value: json!(true),
        }),
        "the condition is on the reading's own field, bare, because the step has one dependency"
    );
}

/// Every action the flow names is one the default registry has.
#[test]
fn every_action_the_flow_names_is_registered() {
    let registry = registry::default_registry(None, None);
    let names = registry.names();
    for step in shipped().graph.steps() {
        assert!(
            names.contains(&step.action.as_str()),
            "«{}» names «{}», which the engine does not have",
            step.id,
            step.action
        );
    }
}

/// The engine is handed the whole fault, told to reproduce it with a test
/// before fixing it, and asked to answer only what it measured.
#[test]
fn the_mandate_carries_the_fault_and_asks_for_a_red_test_and_an_honest_answer() {
    let flow = shipped();
    let stdin = repair_stdin(&flow);
    for pointer in ["/what_happened", "/how_it_showed", "/what_would_prevent", "/happened_on"] {
        assert!(stdin.contains(&format!("\"$from\":\"{pointer}\"")), "{pointer} is carried in");
    }
    assert!(stdin.contains("\"$json\":\"/number\""), "the number travels as JSON, not as text");
    assert!(stdin.contains("/answer_shape"), "the answer's shape is carried in");
    assert!(stdin.contains("see it red on the tree as it is"), "a red test comes before the fix");
    assert!(stdin.contains("Say only what you measured"), "the answer is what was measured");
    let repair = flow.graph.step("repair").expect("the engine step");
    let with = repair.with.as_ref().expect("with");
    assert_eq!(with["data"], json!("private"), "the code repaired is not public text");
    let required = with["answer_shape"]["required"].as_array().expect("required fields");
    for field in ["reproduced", "fixed", "test", "left_open"] {
        assert!(required.iter().any(|name| name == field), "«{field}» is not asked for");
    }
}

/// **WITH NOTHING OPEN THE ENGINE IS NEVER STARTED.** The register is empty,
/// the reading says so, and the condition leaves the step unopened.
#[test]
fn with_nothing_open_the_engine_step_never_opens() {
    let scratch = Scratch::new();
    let (execution, store) = run(&scratch, &shipped().graph);

    let records = store.all();
    let next = records
        .iter()
        .find(|record| record.step_id == "next")
        .expect("the reading ran");
    assert_eq!(next.output.as_ref().and_then(|out| out.get("open")), Some(&json!(false)));
    let repair = records.iter().find(|record| record.step_id == "repair");
    assert!(
        repair.is_none_or(|record| record.outcome == Some(Outcome::Skipped)),
        "the engine was started with nothing to repair: {:?}",
        repair.map(|record| &record.outcome)
    );
    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "the run closes without an engine: {:?}",
        execution.decisions.last()
    );
}

/// The control: one open fault, and the engine step opens with that fault's
/// words in its mandate, so the silence above is the condition at work.
#[test]
fn an_open_fault_becomes_the_engine_s_whole_mandate() {
    let scratch = Scratch::new();
    let fault = an_open_fault(&scratch);
    let (_, store) = run(&scratch, &graph_answering());

    let records = store.all();
    let repair = records
        .iter()
        .find(|record| record.step_id == "repair")
        .expect("the engine step was opened");
    assert_eq!(repair.outcome, Some(Outcome::Went), "{:?}", repair.failure_class);
    let stdin = repair.input["stdin"].as_str().expect("the mandate is text");
    assert!(stdin.contains(&fault.what_happened), "the fault's words are the mandate:\n{stdin}");
    assert!(stdin.contains(&fault.what_would_prevent), "and so is the check it names:\n{stdin}");
    assert!(stdin.contains(&format!("number: {}", fault.number)), "{stdin}");
    let answer = repair.output.as_ref().expect("the engine answered");
    assert_eq!(answer["answer"]["fixed"], json!(true), "{answer}");
}
