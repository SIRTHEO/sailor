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
use serde_json::json;

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

/// The product's registry over a house and a register of this test's own: the
/// machine's register may hold an open fault, and the engine step would start.
fn registry(scratch: &Scratch) -> ActionRegistry {
    let ledger = ledger::Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    let mut registry = registry::registry_in(registry::House::under(&scratch.0), Some(ledger), None);
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

/// The round works in the scratch tree, as a real one works in the checkout:
/// the engine writes there, and the warrant reads there.
fn run(scratch: &Scratch, graph: &Graph) -> (Execution, InMemoryRecordStore) {
    run_accepting(scratch, graph, THE_CHECK)
}

/// The acceptance enters as the trigger's text, the one input a launch sets:
/// it is fixed here, before the engine starts, and the engine never sees it as
/// a field it may answer.
fn run_accepting(scratch: &Scratch, graph: &Graph, check: &str) -> (Execution, InMemoryRecordStore) {
    let mut root_inputs: std::collections::BTreeMap<String, serde_json::Value> =
        shipped().inputs.into_iter().collect();
    root_inputs.get_mut("trigger").expect("the trigger's input")["text"] = json!(check);
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        scratch.0.display().to_string().into(),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: "taken".to_owned(),
        root_inputs,
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry(scratch), &Tick(0.into()))
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
    graph_answering_with(answering())
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
            status: "**open**".to_owned(),
            standing: None,
        })
        .expect("the fault is recorded")
}

/// **THE READING COMES FIRST, AND THE ENGINE WAITS ON IT AND ON THE
/// ACCEPTANCE**: the fault it repairs, and the command that will judge it,
/// fixed by whoever launched; the condition reads the reading's `/next/open`.
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
            ("acceptance", "shell_check"),
            ("repair", "external_engine"),
            ("warrant", "shell_check"),
            ("learn", "remember"),
        ]
    );
    let repair = flow.graph.step("repair").expect("the engine step");
    assert_eq!(repair.deps, vec!["trigger", "next", "acceptance"]);
    assert_eq!(
        repair.when,
        Some(Condition::PointerEquals {
            pointer: "/next/open".to_owned(),
            value: json!(true),
        }),
        "the condition is on the reading's own field"
    );
    let acceptance = flow.graph.step("acceptance").expect("the acceptance step");
    assert_eq!(acceptance.deps, vec!["trigger"], "the acceptance is read from the launch alone");
    assert_eq!(
        acceptance.with.as_ref().expect("with")["env"]["CHECK"],
        json!({"$from": "/text"}),
        "one dependency, so the pointer is bare"
    );
}

/// Every action the flow names is one the default registry has.
#[test]
fn every_action_the_flow_names_is_registered() {
    let registry = registry::registry_in(registry::House::empty(), None, None);
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
    for pointer in [
        "/next/what_happened",
        "/next/how_it_showed",
        "/next/what_would_prevent",
        "/next/happened_on",
        "/trigger/text",
    ] {
        assert!(stdin.contains(&format!("\"$from\":\"{pointer}\"")), "{pointer} is carried in");
    }
    assert!(stdin.contains("\"$json\":\"/next/number\""), "the number travels as JSON, not as text");
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
    assert!(
        with["answer_shape"]["properties"].get("check").is_none(),
        "the answer has no field in which to name the command that judges it"
    );
    let warrant = flow.graph.step("warrant").expect("the warrant step");
    assert_eq!(warrant.deps, vec!["trigger", "repair"]);
    let with = warrant.with.as_ref().expect("with");
    assert_eq!(
        with["env"]["CHECK"],
        json!({"$from": "/trigger/text"}),
        "the check that reaches the warrant is the launch's, not the answer's"
    );
    let command = with["command"].as_str().expect("the warrant is a shell line");
    assert!(command.contains("sh -c \"$CHECK\""), "and the warrant runs it on the tree:\n{command}");
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

// ── what the round leaves for the round after ───────────────────────

/// The engine's answer with a rule drawn from the repair, which is the field
/// the last step is gated on.
fn answering_with_a_rule() -> String {
    json!({
        "reproduced": true,
        "fixed": true,
        "test": "a_missing_price_list_refuses_instead_of_pricing_at_zero, in the pricing crate",
        "changed": "the reader of the price list refuses an empty file",
        "left_open": "",
        "learnt": "a reader that cannot read answers «I do not know», never a zero"
    })
    .to_string()
}

fn graph_answering_with(said: String) -> Graph {
    graph_producing(said, A_RESULT_THE_CHECK_ACCEPTS)
}

/// What the engine leaves on the tree, and the check that reads it there:
/// the whole round is proved by this line and not by the answer.
const THE_RESULT: &str = "repaired.txt";
const THE_CHECK: &str = "grep -q 'refuses an empty file' repaired.txt";
const A_RESULT_THE_CHECK_ACCEPTS: &str = "the reader refuses an empty file";
const A_RESULT_THE_CHECK_REJECTS: &str = "the reader prices an empty file at zero";

/// The engine replaced by a shell that writes `produced` on the tree and
/// prints `said` as its answer; nothing else in the graph is touched.
fn graph_producing(said: String, produced: &str) -> Graph {
    let mut steps: Vec<Step> = shipped().graph.steps().to_vec();
    for step in &mut steps {
        if step.id == "repair" {
            let with = step.with.as_mut().expect("the engine step carries its values");
            // A command line written by hand carries the model in itself, and a
            // step declaring both is refused.
            with.as_object_mut().expect("the values are a map").remove("model");
            with["args"] = json!([
                "-c",
                format!("cat > /dev/null; printf '%s' \"$2\" > {THE_RESULT}; printf '%s' \"$1\""),
                "engine",
                said,
                produced
            ]);
        }
    }
    Graph::new(steps).expect("the graph stays valid")
}

/// The check as a person would run it from the root of the tree.
fn the_check_on(scratch: &Scratch) -> bool {
    std::process::Command::new("sh")
        .args(["-c", THE_CHECK])
        .current_dir(&scratch.0)
        .status()
        .expect("the shell runs")
        .success()
}

// ── the round is proved on the tree, not by the answer ──────────────

/// **A RESULT THE CHECK REJECTS DOES NOT CLOSE THE ROUND.** The engine says
/// «fixed» and leaves a result the check named is red on; the answer is not
/// the proof, the tree is, and a round whose check is red stays open.
#[test]
fn a_result_the_check_rejects_does_not_close_the_round() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (execution, store) = run(
        &scratch,
        &graph_producing(answering(), A_RESULT_THE_CHECK_REJECTS),
    );

    assert!(!the_check_on(&scratch), "the fixture's result is red on the tree");
    let records = store.all();
    let repair = records
        .iter()
        .find(|record| record.step_id == "repair")
        .expect("the engine step was opened");
    assert_eq!(repair.outcome, Some(Outcome::Went), "{:?}", repair.failure_class);
    let warrant = records
        .iter()
        .find(|record| record.step_id == "warrant")
        .expect("the warrant was asked");
    assert_eq!(
        warrant.input["workdir"],
        json!(scratch.0.display().to_string()),
        "the check runs in the tree the engine worked in"
    );
    assert_eq!(
        warrant.outcome,
        Some(Outcome::Broke),
        "the check is red on the tree and the warrant passed on the answer's word: {:?}",
        warrant.failure_class
    );
    assert_eq!(
        flow::run_status(&execution),
        ("failed", false),
        "the round closed on a red check: {:?}",
        execution.decisions.last()
    );
}

/// **AND A ROUND LAUNCHED WITHOUT AN ACCEPTANCE STARTS NO ENGINE.** An empty
/// command exits zero for every result there is; it is refused before any
/// engine is called, naming what to launch with.
#[test]
fn a_round_launched_without_an_acceptance_starts_no_engine() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (execution, store) = run_accepting(
        &scratch,
        &graph_producing(answering(), A_RESULT_THE_CHECK_ACCEPTS),
        "",
    );

    let records = store.all();
    let acceptance = records
        .iter()
        .find(|record| record.step_id == "acceptance")
        .expect("the acceptance was asked");
    assert_eq!(acceptance.outcome, Some(Outcome::Broke), "{:?}", acceptance.failure_class);
    let said = serde_json::to_string(&acceptance).expect("a record serialises");
    assert!(said.contains("sailor flow run take-the-next-fault"), "it says what to launch with: {said}");
    assert!(
        records.iter().all(|record| record.step_id != "repair"),
        "an engine was started with nothing to see it green"
    );
    assert!(
        !std::path::Path::new(&scratch.0).join(THE_RESULT).exists(),
        "and nothing was written on the tree"
    );
    assert!(
        !matches!(execution.decisions.last(), Some(Decision::Complete)),
        "{:?}",
        execution.decisions.last()
    );
}

/// The control: the same engine, a result the check accepts, and the round
/// closes — so the refusal above is the check at work and not the fixture.
#[test]
fn a_result_the_check_accepts_closes_the_round() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (execution, store) = run(&scratch, &graph_answering());

    assert!(the_check_on(&scratch), "the fixture's result is green on the tree");
    let warrant = store
        .all()
        .into_iter()
        .find(|record| record.step_id == "warrant")
        .expect("the warrant was asked");
    assert_eq!(warrant.outcome, Some(Outcome::Went), "{:?}", warrant.failure_class);
    assert_eq!(flow::run_status(&execution), ("complete", true), "{:?}", execution.decisions.last());
}

/// The engine names `true` as the test that proves its work: a command it
/// chose, green on every tree there is.
fn answering_with_its_own_check() -> String {
    answering().replace(
        "a_missing_price_list_refuses_instead_of_pricing_at_zero, in the pricing crate",
        "true",
    )
}

/// **THE ENGINE DOES NOT CHOOSE THE COMMAND THAT JUDGES IT.** The acceptance
/// is fixed before the repair starts, by whoever launched; a result that
/// check rejects stays open whatever command the answer names.
#[test]
fn a_check_the_engine_names_does_not_replace_the_acceptance() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (execution, store) = run(
        &scratch,
        &graph_producing(answering_with_its_own_check(), A_RESULT_THE_CHECK_REJECTS),
    );

    assert!(!the_check_on(&scratch), "the fixture's result is red on the tree");
    let records = store.all();
    let warrant = records
        .iter()
        .find(|record| record.step_id == "warrant")
        .expect("the warrant was asked");
    assert_eq!(
        warrant.outcome,
        Some(Outcome::Broke),
        "the acceptance is red on the tree and the warrant passed on a command the engine chose: {:?}",
        warrant.failure_class
    );
    assert_eq!(
        flow::run_status(&execution),
        ("failed", false),
        "the round closed on a check the engine chose: {:?}",
        execution.decisions.last()
    );
    assert_eq!(
        warrant.input["env"]["CHECK"],
        json!(THE_CHECK),
        "the warrant runs the check that was fixed before the repair, not the one the answer names"
    );
}

/// **A ROUND LEAVES A RULE, NOT ITS OWN SUMMARY.** The rule is written in the
/// same call that repaired — no closing turn is paid for it — and what is kept
/// carries the evidence with it: the test that proves it and what changed.
#[test]
fn a_rule_drawn_from_the_repair_is_kept_with_the_evidence_that_earned_it() {
    let scratch = Scratch::new();
    let fault = an_open_fault(&scratch);
    let (_, store) = run(&scratch, &graph_answering_with(answering_with_a_rule()));

    let records = store.all();
    let learn = records
        .iter()
        .find(|record| record.step_id == "learn")
        .expect("the rule was kept");
    assert_eq!(learn.outcome, Some(Outcome::Went), "{:?}", learn.failure_class);
    let value = learn.input["value"].as_str().expect("what is kept is text");
    assert!(value.contains("never a zero"), "the rule itself: {value}");
    assert!(
        value.contains("a_missing_price_list_refuses_instead_of_pricing_at_zero"),
        "and the test that proves it: {value}"
    );
    assert_eq!(
        learn.input["label"],
        json!(format!("what fault {} taught", fault.number)),
        "the rule is filed under the fault it came from"
    );
}

/// **AND A ROUND THAT LEARNT NOTHING WRITES NOTHING.** Inventing a rule to
/// have one is how a memory fills with sentences nobody can apply, and the
/// engine is told to leave the field out rather than fill it.
#[test]
fn a_repair_that_drew_no_rule_keeps_nothing() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (_, store) = run(&scratch, &graph_answering());

    let records = store.all();
    let learn = records.iter().find(|record| record.step_id == "learn");
    assert!(
        learn.is_none_or(|record| record.outcome == Some(Outcome::Skipped)),
        "no rule offered, nothing kept — and skipped, not broken: {:?}",
        learn.map(|record| (&record.outcome, &record.failure_class))
    );
}

/// The answer that says both things at once: a rule drawn from a repair that
/// reproduced nothing. A strong model reviewing the design named this exact
/// case — an answer needs only to carry `learnt` beside `reproduced: false`
/// for the step to publish a rule the answer itself contradicts.
fn answering_with_a_rule_it_did_not_earn() -> String {
    json!({
        "reproduced": false,
        "fixed": false,
        "test": "none: the check could not be written",
        "changed": "nothing",
        "left_open": "the whole fault",
        "learnt": "a reader that cannot read answers «I do not know», never a zero"
    })
    .to_string()
}

/// **SAILOR READS THE TWO WORDS THE ROUND RESTS ON, NOT THE ENGINE.** Whether
/// the fault was seen red and is green now decides both whether the round
/// closed and whether anything it says may be handed to the next one; leaving
/// it to the prompt makes a rule out of an answer that contradicts itself.
#[test]
fn a_rule_is_not_published_on_a_repair_that_reproduced_nothing() {
    let scratch = Scratch::new();
    let _fault = an_open_fault(&scratch);
    let (_, store) = run(
        &scratch,
        &graph_answering_with(answering_with_a_rule_it_did_not_earn()),
    );

    let records = store.all();
    let warrant = records
        .iter()
        .find(|record| record.step_id == "warrant")
        .expect("the warrant was asked");
    assert_eq!(
        warrant.outcome,
        Some(Outcome::Broke),
        "a repair that reproduced nothing does not pass: {:?}",
        warrant.failure_class
    );
    let learn = records.iter().find(|record| record.step_id == "learn");
    assert!(
        learn.is_none_or(|record| record.outcome != Some(Outcome::Went)),
        "and nothing it said is handed on: {:?}",
        learn.map(|record| &record.outcome)
    );
}
