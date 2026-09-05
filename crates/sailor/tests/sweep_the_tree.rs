//! The shipped flow that gives mechanical work to the local runner and hands
//! the result over as a proposal. No engine is started here: every tool
//! resolves to `sh`, and the steps' arguments are replaced with what prints
//! the answer a real engine would.

use actions::ToolResolver;
use flow::{
    ActionRegistry, Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    Graph, InMemoryRecordStore, InProcessExecutor, SharedState, Step,
};
use serde_json::{json, Value};

const FLOW_ID: &str = "sweep-the-tree";

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

    /// The step's text is private: a fake with no pact would be refused.
    fn data_pact(&self, _id: &str) -> models::pact::DataPact {
        models::pact::DataPact::DoesNotTrain
    }
}

fn registry() -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(EveryToolIsShell),
    );
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new(),
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
    let store = InMemoryRecordStore::default();
    let request = ExecutionRequest {
        run_id: "sweep".to_owned(),
        root_inputs: [("trigger".to_owned(), trigger)].into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
    };
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry(), &Tick(0.into()))
        .expect("the execution does not break");
    (execution, store)
}

/// `sh -c 'printf %s "$1"' - <text>`: whatever the step sends, the answer is `text`.
fn prints(text: &str) -> Value {
    json!(["-c", "cat > /dev/null; printf '%s' \"$1\"", "engine", text])
}

/// The mechanical step goes to the local runner and to nobody paid; it says
/// its kind and that its text is private, and the last step only hands over.
#[test]
fn the_local_runner_is_the_only_engine_and_the_end_is_a_handover() {
    let flow = flow_file();
    assert_eq!(flow.id, FLOW_ID);
    let ids: Vec<&str> = flow.graph.steps().iter().map(|step| step.id.as_str()).collect();
    assert_eq!(ids, vec!["trigger", "read", "rewrite", "hand"]);

    let rewrite = flow.graph.step("rewrite").expect("the step exists");
    let with = rewrite.with.as_ref().expect("it carries its values");
    assert_eq!(with["tool"], json!(["ollama"]), "no paid subscription in the chain");
    assert_eq!(with["kind"], json!("mechanical"));
    assert_eq!(with["data"], json!("private"));
    assert_eq!(flow.graph.step("hand").expect("the step exists").action, actions::handoff::HANDED_TO_AGENT_ACTION);
}

/// Run without spending: the file read is what the model receives, the
/// proposal is what the person receives, and the run waits on them.
#[test]
fn the_proposal_reaches_the_handover_whole_and_the_run_waits_for_a_person() {
    let flow = flow_file();
    let proposal = json!({"content": "fn a() {}\n", "changed": true, "notes": "one comment cut"}).to_string();
    let mut steps: Vec<Step> = flow.graph.steps().to_vec();
    for step in &mut steps {
        let Some(with) = step.with.as_mut() else { continue };
        match step.id.as_str() {
            "read" => with["args"] = prints("// a comment\nfn a() {}\n"),
            "rewrite" => with["args"] = prints(&proposal),
            _ => {}
        }
    }
    let graph = Graph::new(steps).expect("the graph stays valid");
    let mut trigger = flow.inputs["trigger"].clone();
    trigger["where"] = json!("src/a.rs");

    let (execution, store) = run(&graph, trigger);

    assert_eq!(
        execution.decisions.last().cloned().expect("a decision"),
        Decision::Waiting(vec!["hand".to_owned()]),
        "the proposal waits for a person, nothing writes"
    );
    let records = store.all();
    let handed = records
        .iter()
        .find(|record| record.step_id == "hand")
        .expect("the handover was opened");
    let mandate = handed.input["mandate"].as_str().expect("the mandate is text");
    assert!(mandate.contains("src/a.rs") && mandate.contains("fn a() {}") && mandate.contains("one comment cut"), "{mandate}");
    assert!(mandate.contains("cargo test"), "the check is the person's gesture: {mandate}");
}
