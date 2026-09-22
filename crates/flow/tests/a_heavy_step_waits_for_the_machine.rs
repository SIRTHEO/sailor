//! A step declared heavy asks for the machine before its action starts, holds
//! it while the action runs, and hands the turn's token to that action alone.

use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, ExecutionRequest, Executor, Graph,
    HeldTurn, InMemoryRecordStore, InProcessExecutor, MachineTurns, RunStops, SharedState, Step,
    SystemClock, ValueSchema, CURRENT_STEP, MACHINE_TURN,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

fn said() -> &'static Mutex<Vec<String>> {
    static SAID: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    SAID.get_or_init(|| Mutex::new(Vec::new()))
}

fn say(line: String) {
    said().lock().expect("the list").push(line);
}

/// Lets the turn go the moment it is dropped, and says so.
struct Hold(String);

impl Drop for Hold {
    fn drop(&mut self) {
        say(format!("let go {}", self.0));
    }
}

struct FakeMachine;

impl MachineTurns for FakeMachine {
    fn wait_for_the_machine(&self, _run_id: &str, step_id: &str) -> Result<HeldTurn, String> {
        say(format!("took {step_id}"));
        if step_id == "refused" {
            return Err("the machine would not say".to_owned());
        }
        Ok(HeldTurn {
            token: format!("token-{step_id}"),
            hold: Box::new(Hold(step_id.to_owned())),
        })
    }
}

struct Act;

impl Action for Act {
    fn execute(&self, _input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let step = shared.get(CURRENT_STEP).and_then(Value::as_str).unwrap_or("?");
        let token = shared.get(MACHINE_TURN).and_then(Value::as_str).unwrap_or("none");
        say(format!("ran {step} with {token}"));
        Ok(ActionOutcome::Went(json!({})))
    }
}

fn step(id: &str, deps: &[&str], weight: flow::Weight) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: None,
        when: None,
        action: "act".to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        required: false,
        needs: Vec::new(),
        weight,
        at_the_end: false,
    }
}

fn run(steps: Vec<Step>) -> Vec<String> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _alone = ONE_AT_A_TIME.lock().expect("the tests take turns");
    flow::heavy_steps_wait_on(Box::new(FakeMachine));
    said().lock().expect("the list").clear();
    let graph = Graph::new(steps).expect("a sane graph");
    let mut actions = ActionRegistry::default();
    actions.register("act", Act);
    let request = ExecutionRequest {
        holder: None,
        run_id: "run".to_owned(),
        root_inputs: BTreeMap::new(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let store = InMemoryRecordStore::default();
    let _ = InProcessExecutor.execute(&graph, request, &store, &actions, &SystemClock);
    said().lock().expect("the list").clone()
}

#[test]
fn a_heavy_step_holds_the_machine_exactly_while_its_action_runs() {
    let lines = run(vec![
        step("light", &[], flow::Weight::Light),
        step("heavy", &["light"], flow::Weight::Heavy),
        step("after", &["heavy"], flow::Weight::Light),
    ]);

    assert_eq!(
        lines,
        [
            "ran light with none",
            "took heavy",
            "ran heavy with token-heavy",
            "let go heavy",
            "ran after with none",
        ]
    );
}

#[test]
fn a_turn_that_cannot_be_taken_breaks_the_step_without_running_it() {
    let lines = run(vec![step("refused", &[], flow::Weight::Heavy)]);

    assert_eq!(lines, ["took refused"]);
}
