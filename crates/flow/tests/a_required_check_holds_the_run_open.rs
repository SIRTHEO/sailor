//! A run is not done while a step the flow declared `required` has not passed.
//!
//! `decides_done` **permits** an early success; `required` **withholds** an
//! ordinary one. Both read `/status` equal to `passed` and believe nothing else.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Condition, Decision, ExecutionRequest,
    Executor, Graph, InMemoryRecordStore, InProcessExecutor, RunStops, SharedState, Step,
    StopReason, SystemClock, Unmet, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Answers the verdict its input names, and declares itself a check.
struct Verdict;

impl Action for Verdict {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        match input.get("say") {
            Some(Value::String(word)) if word == "break" => {
                Err(ActionError::new("check_failed", "the check exited non-zero"))
            }
            Some(said) => Ok(ActionOutcome::Went(said.clone())),
            None => Ok(ActionOutcome::Went(Value::Null)),
        }
    }

    fn is_a_check(&self) -> bool {
        true
    }
}

/// Hands its input back, and is not a check: it stands in for the engine.
struct Echo;

impl Action for Echo {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

fn step(id: &str, action: &str, deps: &[&str]) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        required: false,
        needs: Vec::new(),
        weight: flow::Weight::Light,
        at_the_end: false,
    }
}

fn registry() -> ActionRegistry {
    let mut actions = ActionRegistry::default();
    actions.register("verdict", Verdict);
    actions.register("echo", Echo);
    actions
}

fn request(root_inputs: BTreeMap<String, Value>) -> ExecutionRequest {
    ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs,
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    }
}

fn run(graph: &Graph, request: ExecutionRequest) -> (Vec<Decision>, (&'static str, bool)) {
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry(), &SystemClock)
        .expect("the run answers");
    let status = flow::run_status(&execution);
    (execution.decisions, status)
}

/// A step of work, then the acceptance the flow cannot complete without, with
/// the verdict that acceptance will answer.
fn work_then_acceptance(word: Value) -> (Graph, ExecutionRequest) {
    let mut acceptance = step("acceptance", "verdict", &["work"]);
    acceptance.required = true;
    let graph = Graph::new(vec![step("work", "echo", &[]), acceptance]).expect("a sane graph");
    let roots = [("work".to_owned(), json!({"say": word}))]
        .into_iter()
        .collect();
    (graph, request(roots))
}

/// The gap this declaration closes: the acceptance said `failed`, every step
/// of the graph ran, and before today the run answered "complete".
#[test]
fn a_required_check_that_did_not_pass_holds_the_run_open() {
    let (graph, request) = work_then_acceptance(json!({"status": "failed"}));
    let (decisions, status) = run(&graph, request);

    assert_eq!(
        decisions.last(),
        Some(&Decision::RequirementUnmet {
            step: "acceptance".to_owned(),
            reason: Unmet::DidNotPass,
        }),
        "a red acceptance closed the run as done: {decisions:?}"
    );
    assert_eq!(status, ("failed", false), "{decisions:?}");
}

/// **TOLERANCE DOES NOT SATISFY A REQUIREMENT.** A step declaring
/// `accept: ["failed"]` writes exactly this output instead of breaking: what a
/// step forgives itself is the step's business, and the run's acceptance is the
/// run's.
#[test]
fn a_check_that_forgave_its_own_failure_still_holds_the_run_open() {
    let tolerated = json!({
        "status": "failed",
        "unresolved": "the check `cargo test -p flow` exited 1; 2 tests failed"
    });
    let (graph, request) = work_then_acceptance(tolerated);
    let (decisions, status) = run(&graph, request);

    assert_eq!(
        decisions.last(),
        Some(&Decision::RequirementUnmet {
            step: "acceptance".to_owned(),
            reason: Unmet::DidNotPass,
        }),
        "a tolerated failure satisfied the requirement: {decisions:?}"
    );
    assert_eq!(status, ("failed", false), "{decisions:?}");
}

/// A required check nobody ever asked. The gate passes, the run would halt as
/// `Checked` and read "complete", and the acceptance has not run at all: an
/// early success is exactly the moment a requirement matters.
#[test]
fn a_required_check_a_passing_gate_left_unasked_holds_the_run_open() {
    let mut gate = step("gate", "verdict", &[]);
    gate.decides_done = true;
    let mut acceptance = step("acceptance", "verdict", &["work"]);
    acceptance.required = true;
    let graph = Graph::new(vec![gate, step("work", "echo", &["gate"]), acceptance])
        .expect("a sane graph");
    let roots = [("gate".to_owned(), json!({"say": {"status": "passed"}}))]
        .into_iter()
        .collect();
    let (decisions, status) = run(&graph, request(roots));

    assert!(
        !decisions.iter().any(|decision| matches!(
            decision,
            Decision::Halted {
                reason: StopReason::Checked,
                ..
            }
        )),
        "a gate closed the run while the acceptance was still unasked: {decisions:?}"
    );
    assert_eq!(status.0, "failed", "{decisions:?}");
}

/// A required check its own `when` judged false. The author wrote the
/// condition guarding it, and nothing it guards happened: the requirement is
/// waived, not unmet.
#[test]
fn a_required_check_its_own_condition_ruled_out_completes_the_run() {
    let mut acceptance = step("acceptance", "verdict", &["work"]);
    acceptance.required = true;
    acceptance.when = Some(Condition::PointerEquals {
        pointer: "/ready".to_owned(),
        value: json!(true),
    });
    let graph = Graph::new(vec![step("work", "echo", &[]), acceptance]).expect("a sane graph");
    let roots = [("work".to_owned(), json!({"ready": false}))]
        .into_iter()
        .collect();
    let (decisions, status) = run(&graph, request(roots));

    assert_eq!(
        decisions.last(),
        Some(&Decision::Complete),
        "a condition ruled out by its own author held the run open: {decisions:?}"
    );
    assert_eq!(status, ("complete", true), "{decisions:?}");
}

/// A required check nothing ever opened, because the step before it was ruled
/// out by its own condition and took its dependency with it. Nothing the
/// requirement guards happened here either: waived the same way, one hop
/// removed.
#[test]
fn a_required_check_left_unopened_by_an_upstream_condition_completes_the_run() {
    let mut work = step("work", "echo", &[]);
    work.when = Some(Condition::PointerEquals {
        pointer: "/ready".to_owned(),
        value: json!(true),
    });
    let mut acceptance = step("acceptance", "verdict", &["work"]);
    acceptance.required = true;
    let graph = Graph::new(vec![work, acceptance]).expect("a sane graph");
    let roots = [("work".to_owned(), json!({"ready": false}))]
        .into_iter()
        .collect();
    let (decisions, status) = run(&graph, request(roots));

    assert_eq!(
        decisions.last(),
        Some(&Decision::Complete),
        "an acceptance left unopened by an upstream condition held the run open: {decisions:?}"
    );
    assert_eq!(status, ("complete", true), "{decisions:?}");
}

/// A required step never reached because what stood between it and the root
/// genuinely broke — not a condition anybody wrote, an action that ran out of
/// attempts. The run must not read this as waived: it ends on the broken step's
/// own name, same as today, and never as complete.
#[test]
fn a_required_check_left_unopened_by_a_real_failure_does_not_complete_the_run() {
    let mut gate = step("gate", "verdict", &[]);
    gate.max_attempts = 1;
    let mut acceptance = step("acceptance", "verdict", &["gate"]);
    acceptance.required = true;
    let graph = Graph::new(vec![gate, acceptance]).expect("a sane graph");
    let roots = [("gate".to_owned(), json!({"say": "break"}))]
        .into_iter()
        .collect();
    let (decisions, status) = run(&graph, request(roots));

    assert_eq!(
        decisions.last(),
        Some(&Decision::Failed(vec!["gate".to_owned()])),
        "a real failure upstream of a required step read as complete: {decisions:?}"
    );
    assert_eq!(status, ("failed", false), "{decisions:?}");
}

/// A required check that broke needs no new word: the run already ends on the
/// step whose attempts are spent, and that decision names it.
#[test]
fn a_required_check_that_broke_ends_the_run_on_its_own_name() {
    let (graph, request) = work_then_acceptance(json!("break"));
    let (decisions, status) = run(&graph, request);

    assert_eq!(
        decisions.last(),
        Some(&Decision::Failed(vec!["acceptance".to_owned()])),
        "{decisions:?}"
    );
    assert_eq!(status, ("failed", false));
}

/// And the whole point of declaring one: it passes, and the run is done exactly
/// as it was before this existed.
#[test]
fn a_required_check_that_passed_completes_the_run() {
    let (graph, request) = work_then_acceptance(json!({"status": "passed"}));
    let (decisions, status) = run(&graph, request);

    assert_eq!(decisions.last(), Some(&Decision::Complete), "{decisions:?}");
    assert_eq!(status, ("complete", true));
}

/// A flow declaring nothing required ends where it always ended, red verdict
/// and all. Without this the tests above would pass on a rule that had simply
/// stopped every run.
#[test]
fn a_flow_with_no_required_step_ends_as_it_always_did() {
    for word in [json!({"status": "failed"}), json!({"status": "passed"})] {
        let acceptance = step("acceptance", "verdict", &["work"]);
        let graph =
            Graph::new(vec![step("work", "echo", &[]), acceptance]).expect("a sane graph");
        let roots = [("work".to_owned(), json!({"say": word.clone()}))]
            .into_iter()
            .collect();
        let (decisions, status) = run(&graph, request(roots));

        assert_eq!(decisions.last(), Some(&Decision::Complete), "«{word}»");
        assert_eq!(status, ("complete", true), "«{word}»");
    }
}

/// The key is the flow's, not only the executor's: `flow check` and every
/// reading of a flow file go through this, and an unknown spelling is refused
/// here as every other unknown field is.
#[test]
fn the_flow_file_carries_the_key_and_refuses_another_spelling() {
    let mut written = json!({
        "id": "acceptance",
        "deps": [],
        "input_schema": {"type": "any"},
        "output_schema": {"type": "any"},
        "when": null,
        "action": "verdict",
        "max_attempts": 1
    });

    let silent: Step = serde_json::from_value(written.clone()).expect("a step declaring nothing");
    assert!(!silent.required, "absent means not required");

    written["required"] = json!(true);
    let declared: Step = serde_json::from_value(written.clone()).expect("the key is the flow's");
    assert!(declared.required);
    assert_eq!(
        serde_json::to_value(&declared).expect("a step is written back")["required"],
        json!(true),
        "a declared requirement has to survive being written down"
    );

    let mut misspelt = written.clone();
    misspelt.as_object_mut().expect("an object").remove("required");
    misspelt["require"] = json!(true);
    let refused =
        serde_json::from_value::<Step>(misspelt).expect_err("a spelling nobody declared");
    assert!(refused.to_string().contains("require"), "{refused}");
}
