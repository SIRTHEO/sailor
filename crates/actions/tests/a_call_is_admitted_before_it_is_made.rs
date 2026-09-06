//! The control before the call, proved on a run that would otherwise spend.
//!
//! A cap used to stop a run once the spend had already reached it: it could
//! hold back the call after, never the overshoot of the one it let through.
//! Here the condition is checked per call, with a fake engine and a fake price
//! list under `temp_dir`. Nothing real is called.

use actions::reserve::{CeilingOption, UNIT_CURRENCY};
use actions::{AskRecipe, Declared, ExternalEngineAction, Pointer, PromptVia, Shape, ToolResolver};
use actions::{Reports, UsageRecipe};
use flow::{
    Action, ActionRegistry, ExecutionRequest, Executor, Graph, InProcessExecutor, RecordStore,
    RunStops, SharedState, Step, SystemClock, ValueSchema,
};
use ledger::Ledger;
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("actions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the scratch directory is created");
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A call of 100.000 input tokens, which the price list below puts at 0.30. It
/// writes down the line it was invoked with, so the ceiling reserved can be
/// checked against the ceiling actually imposed.
const A_SMALL_CALL: &str = r#"cat > /dev/null
printf '%s\n' "$@" > "$(dirname "$0")/argv"
printf '{"result":"fatto","model":"modello-di-prova","usage":{"input_tokens":100000,"output_tokens":0}}'"#;

/// The same call, from a model the price list below does not carry. Its tokens
/// are counted and its cost is not: the row it leaves has no cost at all.
const A_CALL_NOBODY_CAN_PRICE: &str = r#"cat > /dev/null
printf '{"result":"fatto","model":"modello-fuori-listino","usage":{"input_tokens":100000,"output_tokens":0}}'"#;

const PRICE_LIST: &str = r#"{
  "currency": "USD",
  "models": [
    { "id": "modello-di-prova", "input_per_million": 3.0, "output_per_million": 15.0 }
  ]
}"#;

fn fake_engine(dir: &Path, name: &str, script: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("the fake engine is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("and made executable");
    path.to_string_lossy().into_owned()
}

/// The shared state the executor prepares before an action, under a cap.
fn capped(step: &str, cap_micros: i64) -> SharedState {
    let mut state = SharedState::new();
    state.insert(flow::CURRENT_RUN.to_owned(), json!("la-corsa"));
    state.insert(flow::CURRENT_STEP.to_owned(), json!(step));
    state.insert(flow::CURRENT_CAP.to_owned(), json!(cap_micros));
    state
}

fn at(keys: &[&str]) -> Option<Pointer> {
    Some(Pointer::Path(keys.iter().map(|k| (*k).to_owned()).collect()))
}

/// An engine that says what it consumed, so a run's spend is known and not a
/// floor: without this the arithmetic under test could not be done at all.
fn declaring_recipe() -> AskRecipe {
    AskRecipe {
        args: Vec::new(),
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: Vec::new(),
        silent_without_prompt: false,
        refuses_without_prompt: Vec::new(),
        exhausted_when: Vec::new(),
        cooldown_secs: None,
        waits_for_a_person_when: Vec::new(),
        usage: Some(UsageRecipe {
            args: Vec::new(),
            declared: Declared {
                read: Shape::Json,
                from: models::usage::Heard::Stdout,
                reports: Reports::PerCall,
                input_tokens: at(&["usage", "input_tokens"]),
                output_tokens: at(&["usage", "output_tokens"]),
                cached_tokens: None,
                cache_write_tokens: None,
                cache_write_long_tokens: None,
                total_tokens: None,
                turns: None,
                cost: None,
                model: at(&["model"]),
                answer: at(&["result"]),
            },
        }),
    }
}

/// A resolver for one engine, which may or may not declare how it is told the
/// most one call may spend. That single fact is what this test turns on.
struct OneEngine {
    bin: String,
    holds_to_a_ceiling: bool,
}

impl ToolResolver for OneEngine {
    fn resolve(&self, id: &str) -> Result<String, String> {
        match id {
            "motore-di-prova" => Ok(self.bin.clone()),
            other => Err(format!("«{other}» is not on this machine")),
        }
    }
    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        Some(declaring_recipe())
    }
    fn spend_ceiling_option(&self, _id: &str) -> Option<CeilingOption> {
        self.holds_to_a_ceiling.then(|| CeilingOption {
            args: vec!["--max-budget-usd".to_owned()],
            unit: UNIT_CURRENCY.to_owned(),
        })
    }
}

/// The price list lives in a file and the tests must not read the home of
/// whoever runs them. One list serves both tests, so the process variable is
/// written once and never contended.
fn with_the_price_list<T>(path: &Path, body: impl FnOnce() -> T) -> T {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    std::env::set_var("SAILOR_PRICING", path);
    let out = body();
    std::env::remove_var("SAILOR_PRICING");
    out
}

fn calls_in(dir: &Path) -> usize {
    let ledger = Ledger::open(dir).expect("the ledger reopens");
    let dump = ledger.projection_dump().expect("it says what it holds");
    ui::parse::parse_model_calls(&dump).len()
}

fn a_run(dir: &Path, holds_to_a_ceiling: bool, ledger: &str) -> ExternalEngineAction {
    ExternalEngineAction::resolving_with(OneEngine {
        bin: fake_engine(dir, "motore", A_SMALL_CALL),
        holds_to_a_ceiling,
    })
    .recording_to(Some(Ledger::open(dir.join(ledger)).expect("a scratch ledger")))
    .budgeted_by(None)
}

/// **THE CONTROL BEFORE THE CALL, NOT AFTER THE SPEND.** A run capped at 1.00
/// whose every call is held to 0.60: two calls at 0.30 fit, the third does not,
/// and the older rule — «stop once the spend reaches the cap» — would have
/// started it, 0.60 being under 1.00. The refusal says both numbers. *Mutant
/// run*: drop the `authorise` call before `ask` in `engine.rs`, and three rows
/// land in the ledger instead of two.
#[test]
fn a_call_whose_reserve_does_not_fit_the_remainder_never_starts() {
    let dir = Scratch::new("ammissione");
    let prices = dir.0.join("pricing.json");
    fs::write(&prices, PRICE_LIST).expect("the fake price list is written");
    let action = a_run(&dir.0, true, "deposito");
    let input = json!({
        "tool": "motore-di-prova",
        "stdin": "ciao",
        "timeout_secs": 10,
        "max_spend_micros": 600_000
    });

    with_the_price_list(&prices, || {
        action
            .execute(&input, &capped("passo-1", 1_000_000))
            .expect("the first call fits the remainder");
        let line = fs::read_to_string(dir.0.join("argv")).expect("the engine wrote its line");
        assert!(
            line.contains("--max-budget-usd") && line.contains("0.600000"),
            "the ceiling reserved is the ceiling imposed: {line}"
        );
        action
            .execute(&input, &capped("passo-2", 1_000_000))
            .expect("the second still fits");
        let refused = action
            .execute(&input, &capped("passo-3", 1_000_000))
            .expect_err("0.60 does not fit the 0.40 left");

        assert_eq!(refused.class, "spend_cap_admission");
        assert!(
            refused.said.contains("0.40") && refused.said.contains("0.60"),
            "both numbers are there: {}",
            refused.said
        );
    });
    assert_eq!(
        calls_in(&dir.0.join("deposito")),
        2,
        "the refused call spent nothing"
    );
}

/// The other half, and the one that holds today: with nothing to reserve
/// against, the same run is not suspended before the remainder is gone — and
/// when it is, the refusal says the reserve could not be known instead of
/// showing a figure that would be invented.
#[test]
fn a_call_nobody_can_reserve_says_so_instead_of_showing_a_number() {
    let dir = Scratch::new("senza-riserva");
    let prices = dir.0.join("pricing.json");
    fs::write(&prices, PRICE_LIST).expect("the fake price list is written");
    let action = a_run(&dir.0, false, "deposito");
    let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

    with_the_price_list(&prices, || {
        // Three calls at 0.30 fill a cap of 0.90 exactly; the fourth finds
        // nothing left, which is the only moment such a cap bites.
        for step in ["passo-1", "passo-2", "passo-3"] {
            action
                .execute(&input, &capped(step, 900_000))
                .expect("under an unreservable cap the older rule still applies");
        }
        let refused = action
            .execute(&input, &capped("passo-4", 900_000))
            .expect_err("nothing is left");

        assert_eq!(refused.class, "spend_cap_admission");
        assert!(
            refused.said.contains("stop threshold")
                && refused.said.contains(actions::reserve::NATIVE_SPEND_CAP),
            "it names what is missing rather than inventing a reserve: {}",
            refused.said
        );
    });
}

fn a_step(id: &str, deps: &[&str], with: Option<serde_json::Value>) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with,
        when: None,
        action: actions::EXTERNAL_ENGINE_ACTION.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }
}

/// **THE RUN THAT OVERSHOT ITS CAP, IN MINIATURE.** Two calls of one engine
/// under a cap of 6.00, and the model that answers is deliberately absent from
/// the price list: its row lands with no cost, the run's spend reads `AtLeast`,
/// and the older rule — «stop when the remainder is gone» — could never see a
/// remainder fall. The second call must not be made, and the store must say
/// why with the figures. *Mutant run*: put `_ => remaining > 0` back as the
/// last arm of `reserve::admits` and both calls go through.
#[test]
fn a_model_the_price_list_cannot_price_stops_the_run_and_the_store_says_why() {
    let dir = Scratch::new("fuori-listino");
    let prices = dir.0.join("pricing.json");
    fs::write(&prices, PRICE_LIST).expect("the fake price list is written");
    let ledger = Ledger::open(dir.0.join("deposito")).expect("a scratch ledger");
    let action = ExternalEngineAction::resolving_with(OneEngine {
        bin: fake_engine(&dir.0, "motore", A_CALL_NOBODY_CAN_PRICE),
        holds_to_a_ceiling: true,
    })
    .recording_to(Some(ledger.clone()))
    .budgeted_by(None);
    let mut registry = ActionRegistry::default();
    registry.register(actions::EXTERNAL_ENGINE_ACTION, action);
    let asked = json!({
        "tool": "motore-di-prova",
        "stdin": "ciao",
        "timeout_secs": 10,
        "max_spend_micros": 600_000
    });
    let graph = Graph::new(vec![
        a_step("uno", &[], None),
        a_step("due", &["uno"], Some(asked.clone())),
    ])
    .expect("a sane graph");

    with_the_price_list(&prices, || {
        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "la-corsa".to_owned(),
                    root_inputs: [("uno".to_owned(), asked.clone())].into_iter().collect(),
                    gates: Vec::new(),
                    shared: SharedState::new(),
                    spend_cap_micros: Some(6_000_000),
                    stops: RunStops::default(),
                },
                &ledger,
                &registry,
                &SystemClock,
            )
            .expect("the run answers");
    });

    assert_eq!(
        calls_in(&dir.0.join("deposito")),
        1,
        "the second call was never made"
    );
    let spent = ledger
        .spent_in_run("la-corsa")
        .expect("the store says what the run spent");
    assert_eq!((spent.calls, spent.calls_without_cost), (1, 1));

    let records = RecordStore::records(&ledger, "la-corsa").expect("the run's records are read");
    let stopped = records
        .iter()
        .find(|record| record.step_id == "due")
        .expect("the second step was opened and closed");
    assert_eq!(stopped.outcome, Some(flow::Outcome::Broke));
    assert_eq!(stopped.failure_class.as_deref(), Some("spend_cap_admission"));
    let said = stopped.said.clone().unwrap_or_default();
    for figure in ["0.00", "6.00", "1 of the 1", "0.60"] {
        assert!(said.contains(figure), "«{figure}» is missing from: {said}");
    }
}
