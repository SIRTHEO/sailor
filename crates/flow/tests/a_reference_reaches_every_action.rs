//! A reference reaches *every* action already resolved, unwritten ones included
//! — fault 28, where the call sat in two actions of nine and a step storing a
//! value from the step before got `{"$from": …}` as an object and died; then in
//! twelve of sixteen, which shrank the symptom and not the fault, being fault
//! 10 in twelve copies with four actions still without. The mutant is removing
//! the call in `step_input`; each test says below what that does to it.

use flow::{
    Action, ActionError, ActionOutcome, Clock, Decision, Executor, FlowError, Graph,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState, Step, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// An action that does *not* know what a reference is: it records the input it
/// receives and returns it unchanged. It is the model of every action registered
/// tomorrow, and that is what this file proves — an input arriving resolved is
/// no merit of a single action but how steps hand each other information, in
/// `flow::step_input`, the one point every step of every run crosses. Were the
/// rule to move back inside the actions, this one would go without, and go red.
struct KeepsWhatItGets(Arc<Mutex<Vec<Value>>>);

impl Action for KeepsWhatItGets {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(input.clone());
        // Returning the input rather than a constant is not laziness: a step's
        // input *is* its dependency's output, so the value the first step
        // declares reaches the second — the road the fault was paid on.
        Ok(ActionOutcome::Went(input.clone()))
    }

    fn species(&self) -> flow::StepSpecies {
        flow::StepSpecies::Repeatable
    }
}

struct Tick(AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

fn step(id: &str, deps: &[&str], with: Option<Value>) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        action: "keeps-what-it-gets".to_owned(),
        max_attempts: 1,
        when: None,
        with,
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
    }
}

/// The same step, with the condition `flows/chiedi-all-indice.flow.json` puts
/// on its `chiedi` and `leggi` steps: it runs only if what it received says
/// `ok`.
fn step_only_when_ok(id: &str, deps: &[&str], with: Option<Value>, pointer: &str) -> Step {
    let mut step = step(id, deps, with);
    step.when = Some(
        serde_json::from_value(json!({
            "kind": "pointer_equals", "pointer": pointer, "value": "ok"
        }))
        .expect("valid condition"),
    );
    step
}

/// Runs the graph and returns the inputs the action saw, in order.
fn what_the_action_saw(graph: &Graph, root_inputs: BTreeMap<String, Value>) -> Vec<Value> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("keeps-what-it-gets", KeepsWhatItGets(seen.clone()));
    let mut store = InMemoryRecordStore::default();
    InProcessExecutor
        .execute(
            graph,
            flow::ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("the run reaches the end");
    let saw = seen.lock().unwrap_or_else(|held| held.into_inner()).clone();
    saw
}

/// The original fault, against an action that resolves nothing: the second step
/// takes from the first the key it stores under, so an unresolved reference
/// hands the action `{"$from": "/stdout"}` as an object and it dies on "invalid
/// type: map, expected a string", `key` wanting text. The store could not carry
/// the baton between two steps. With the mutant: `key` arrives as an object and
/// the assertion falls, saying `{"$from":"/stdout"}` not `yesterdays-work`.
#[test]
fn an_action_that_resolves_nothing_still_gets_its_references_resolved() {
    let graph = Graph::new(vec![
        step("first", &[], None),
        step(
            "second",
            &["first"],
            Some(json!({"collection": "briefs", "key": {"$from": "/stdout"}})),
        ),
    ])
    .expect("valid graph");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert("first".to_owned(), json!({"stdout": "yesterdays-work"}));

    let saw = what_the_action_saw(&graph, root_inputs);

    assert_eq!(saw.len(), 2, "two steps ran: {saw:?}");
    assert_eq!(
        saw[1]["key"],
        json!("yesterdays-work"),
        "the action received the reference instead of the value: {}",
        saw[1]
    );
    assert_eq!(saw[1]["collection"], json!("briefs"));
}

/// `$join` and `$json` travel the same road: the test does not sit on `$from`
/// alone, or half the syntax would stay uncovered.
///
/// With the mutant: `stdin` stays an object and the text comparison falls.
#[test]
fn the_other_two_forms_of_reference_arrive_resolved_too() {
    let graph = Graph::new(vec![
        step("first", &[], None),
        step(
            "second",
            &["first"],
            Some(json!({
                "stdin": {"$join": ["Do only your own section.\n", {"$from": "/stdout"}]},
                "shape_as_text": {"$json": "/answer_shape"},
            })),
        ),
    ])
    .expect("valid graph");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert(
        "first".to_owned(),
        json!({"stdout": "count the dead hooks", "answer_shape": {"type": "number"}}),
    );

    let saw = what_the_action_saw(&graph, root_inputs);

    assert_eq!(
        saw[1]["stdin"],
        json!("Do only your own section.\ncount the dead hooks")
    );
    assert_eq!(saw[1]["shape_as_text"], json!("{\"type\":\"number\"}"));
}

/// A skipped step resolves nothing, and so does not break. Not a schoolbook
/// case: `flows/chiedi-all-indice.flow.json` has step `leggi` with a `when` on
/// `/status`, a `with` full of `$from` into the output of `chiedi`, and `chiedi`
/// among its `skippable_dependencies`. Resolving before the condition took that
/// flow, on the real binary, from "complete" to "failed —
/// `unresolved_reference`".
#[test]
fn a_step_that_does_not_run_never_pays_for_its_references() {
    let graph = Graph::with_skippable_dependencies(
        vec![
            step("guard", &[], None),
            step_only_when_ok("ask", &["guard"], None, "/status"),
            // A skippable dependency arrives *named*: the input is
            // `{"ask": …}` when it is there and `{}` when it was skipped, which
            // is why the pointer carries the step's name.
            step_only_when_ok(
                "read",
                &["ask"],
                Some(json!({"stdin": {"$from": "/ask/said"}})),
                "/ask/status",
            ),
        ],
        [flow::DependencyEdge::new("read", "ask")],
    )
    .expect("valid graph");

    // The index does not answer — the case that flow calls its most frequent.
    // `ask` is skipped, `read` receives `{}` plus its own `with`, and its
    // `$from` finds nothing: it must be skipped. The property needs a test of
    // its own because on that flow the bug was masked — `verdetto`, in the same
    // front, broke first and the run died there, so it was hidden by an
    // accident rather than by a property.
    let (decisions, records) = run_and_read(&graph, "not-ready");
    assert_eq!(
        decisions.last(),
        Some(&Decision::Complete),
        "a skipped step is not a red: {decisions:?}"
    );
    for step_id in ["ask", "read"] {
        let outcome = closed_outcome(&records, step_id);
        assert_eq!(
            outcome,
            Some(Outcome::Skipped),
            "\"{step_id}\" should have been skipped, not {outcome:?}"
        );
    }

    // And the opposite direction, because one alone would prove nothing: when
    // the index answers, the same step runs *and* the reference arrives
    // resolved. The mutant that resolves before the condition fails the half
    // above; the mutant that never resolves at all fails this one.
    let (decisions, records) = run_and_read(&graph, "ok");
    assert_eq!(decisions.last(), Some(&Decision::Complete));
    let read = records
        .iter()
        .find(|record| record.step_id == "read" && record.outcome == Some(Outcome::Went))
        .expect("with the index ready the step runs");
    assert_eq!(
        read.input["stdin"],
        json!("the index's answer"),
        "the reference should have arrived resolved: {}",
        read.input
    );
}

/// Runs the graph with whatever the guard declares, and returns the decisions
/// and the records.
fn run_and_read(graph: &Graph, guard_says: &str) -> (Vec<Decision>, Vec<flow::StepRecord>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("keeps-what-it-gets", KeepsWhatItGets(seen));
    let mut store = InMemoryRecordStore::default();
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert(
        "guard".to_owned(),
        json!({"status": guard_says, "said": "the index's answer"}),
    );
    let execution = InProcessExecutor
        .execute(
            graph,
            flow::ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("the run must not break");
    let records = store.records("run").expect("the run's records");
    (execution.decisions, records)
}

fn closed_outcome(records: &[flow::StepRecord], step_id: &str) -> Option<Outcome> {
    records
        .iter()
        .filter(|record| record.step_id == step_id)
        .find_map(|record| record.outcome.clone())
}

/// A pointer that finds nothing breaks that step, and only that one. Stronger
/// than before on one side — the action is not invoked at all, where resolution
/// inside `external_engine` had the step enter the action to die there — and
/// identical on the other: the defect stays the step's. With the mutant that
/// drops resolution: no broken step, the action is invoked and receives the
/// object, and all of it falls.
#[test]
fn a_pointer_that_finds_nothing_breaks_that_step_and_only_that_one() {
    let graph = Graph::new(vec![
        step("first", &[], None),
        step(
            "second",
            &["first"],
            Some(json!({"stdin": {"$from": "/does/not/exist"}})),
        ),
    ])
    .expect("valid graph");
    let mut root_inputs = BTreeMap::new();
    root_inputs.insert("first".to_owned(), json!({"stdout": "something"}));

    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut actions = flow::ActionRegistry::default();
    actions.register("keeps-what-it-gets", KeepsWhatItGets(seen.clone()));
    let mut store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &graph,
            flow::ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs,
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
            },
            &mut store,
            &actions,
            &Tick(AtomicI64::new(0)),
        )
        .expect("a broken step is not a fault of the run");

    // This half is a repair, not an observation. The first attempt propagated
    // the error with a `?` out of `execute`: the run died opening and closing
    // nothing — no record, no decision, and a step that for the store had never
    // existed. `dispatch_the_work` caught it, demanding `Failed(["verdict"])`.
    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Failed(vec!["second".to_owned()])),
        "the run must say which step broke: {:?}",
        execution.decisions
    );
    assert_eq!(
        seen.lock().unwrap_or_else(|held| held.into_inner()).len(),
        1,
        "only the first step should have run: the second must not even be invoked"
    );

    let broken = store
        .records("run")
        .expect("the run's records")
        .into_iter()
        .find(|record| record.step_id == "second" && record.outcome == Some(Outcome::Broke))
        .expect("the broken step is in the store, or a resume would not know where to restart");
    assert_eq!(broken.failure_class.as_deref(), Some("unresolved_reference"));
    assert!(
        broken.said.as_deref().is_some_and(|said| said.contains("/does/not/exist")),
        "the message must name the pointer to fix: {:?}",
        broken.said
    );
    // The intent keeps the pointer as it was written: whoever reads the record
    // must see what to fix, not the hole it left.
    assert_eq!(broken.input["stdin"], json!({"$from": "/does/not/exist"}));
}
