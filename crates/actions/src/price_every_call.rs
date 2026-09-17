//! The node that gives every call of the ledger an equivalent cost, by the
//! rules the price list pins, and says which calls it still could not price.

use flow::{Action, ActionError, ActionOutcome, RedoEvidence, SharedState, StepSpecies};
use ledger::{Ledger, ModelCallRecord};
use models::pricing::{equivalent_cost, CallFacts, PriceList, TokenCounts};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PRICE_EVERY_CALL_ACTION: &str = "price_every_call";
/// The count the verdict reads, from a step of its own: whoever prices does
/// not also say how many are left.
pub const CALLS_WITHOUT_COST_ACTION: &str = "calls_without_cost";

const KNOWN_FIELDS: &[&str] = &["price_list"];

/// Registered even without a store, so `flow check` can name the action; a
/// run without one refuses, since there is nothing to price.
pub fn register_price_every_call(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(PRICE_EVERY_CALL_ACTION, PriceEveryCallAction { ledger: ledger.clone() });
    registry.register(CALLS_WITHOUT_COST_ACTION, CallsWithoutCostAction { ledger });
}

struct CallsWithoutCostAction {
    ledger: Option<Ledger>,
}

impl Action for CallsWithoutCostAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let Some(ledger) = &self.ledger else {
            return Err(ActionError::new(
                "no_store",
                "counting the calls needs the store they are written in, and this run has none",
            ));
        };
        let remaining = ledger
            .model_calls_without_cost()
            .map_err(|error| ActionError::new("store_failed", error.to_string()))?
            .len();
        Ok(ActionOutcome::Went(json!({ "remaining": remaining })))
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> RedoEvidence {
        RedoEvidence::TouchesNothing
    }
}

/// At run time the input is the trigger's output too, so foreign fields are
/// the norm here; a hand-written `with` is judged by `unknown_fields`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Spec {
    /// A price list to apply exactly as written, instead of the machine's.
    price_list: Option<String>,
}

struct PriceEveryCallAction {
    ledger: Option<Ledger>,
}

fn facts_of(call: &ModelCallRecord) -> CallFacts<'_> {
    CallFacts {
        cli: &call.cli,
        model: Some(call.actual_model.as_str()).filter(|name| !name.trim().is_empty()),
        requested_model: Some(call.requested_model.as_str()).filter(|name| !name.trim().is_empty()),
        counts: TokenCounts {
            input: call.input_tokens,
            output: call.output_tokens,
            cached: call.cached_tokens,
            cache_write: call.cache_write_tokens,
            cache_write_long: call.cache_write_long_tokens,
        },
        // The row keeps one model name and the whole call's counts; how many
        // models were counted apart is not on it, so the row is read as one.
        models_named: None,
        error_type: call.error_type.as_deref(),
        started_at: call.started_at,
        ended_at: call.ended_at,
        declared_cost: call
            .declared_cost_micros
            .map(|micros| micros as f64 / 1_000_000.0),
    }
}

/// Every call without a cost, priced where a rule fits and written back as the
/// same row with the figure and the prices that made it.
pub fn price_every_call(ledger: &Ledger, list: &PriceList) -> Result<Value, ActionError> {
    let calls = ledger.model_calls_without_cost().map_err(store_failed)?;
    price_calls(ledger, list, &calls)
}

/// Every call priced by a rule of the list, including the ones already carrying
/// a figure from an older list, written back only where the figure changes.
pub fn reprice_every_call(ledger: &Ledger, list: &PriceList) -> Result<Value, ActionError> {
    let mut calls = ledger.model_calls_priced_by_rule().map_err(store_failed)?;
    calls.extend(ledger.model_calls_without_cost().map_err(store_failed)?);
    price_calls(ledger, list, &calls)
}

fn store_failed(error: ledger::LedgerError) -> ActionError {
    ActionError::new("store_failed", error.to_string())
}

/// The model a row's step asked its engine for, read from the flow definition
/// the run recorded, for a row written before that model went on the row. Two
/// recorded definitions that disagree leave it unknown.
fn requested_in_recorded_definition(ledger: &Ledger, call: &ModelCallRecord) -> Option<String> {
    if !call.requested_model.trim().is_empty() {
        return None;
    }
    let step = call.step_id.as_deref()?;
    let ledger::RunFlow::Recorded(definitions) = ledger.flow_of_run(&call.run_id).ok()? else {
        return None;
    };
    let mut asked = definitions.iter().map(|definition| {
        let body: Value = serde_json::from_str(&definition.body).ok()?;
        body["graph"]["steps"]
            .as_array()?
            .iter()
            .find(|candidate| candidate["id"] == step)?["with"]["model"][&call.cli]
            .as_str()
            .map(str::to_owned)
    });
    let first = asked.next()??;
    asked.all(|other| other.as_deref() == Some(first.as_str())).then_some(first)
}

fn price_calls(ledger: &Ledger, list: &PriceList, calls: &[ModelCallRecord]) -> Result<Value, ActionError> {
    let mut by_rule: BTreeMap<String, u64> = BTreeMap::new();
    let mut unpriced = Vec::new();
    let mut priced_micros: i64 = 0;
    let mut changed: u64 = 0;
    for call in calls {
        let recovered;
        let call = match requested_in_recorded_definition(ledger, call) {
            Some(model) => {
                recovered = ModelCallRecord {
                    requested_model: model,
                    ..call.clone()
                };
                &recovered
            }
            None => call,
        };
        let priced = equivalent_cost(&facts_of(call), list);
        let Some(micros) = priced.cost_micros else {
            if call.cost_micros.is_some() {
                continue;
            }
            unpriced.push(json!({
                "call_id": call.call_id,
                "cli": call.cli,
                "run_id": call.run_id,
                "step_id": call.step_id,
                "model": call.actual_model,
                "why": "no rule of the price list fits this call",
            }));
            continue;
        };
        let rule = serde_json::to_value(priced.rule)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        *by_rule.entry(rule).or_default() += 1;
        priced_micros += micros;
        let repriced = ModelCallRecord {
            cost_micros: Some(micros),
            price_currency: Some(list.currency.clone()),
            input_price_micros_per_million: priced.prices.input,
            output_price_micros_per_million: priced.prices.output,
            cached_price_micros_per_million: priced.prices.cached,
            cache_write_price_micros_per_million: priced.prices.cache_write,
            cache_write_long_price_micros_per_million: priced.prices.cache_write_long,
            ..call.clone()
        };
        if repriced != *call {
            ledger.record_model_call(&repriced).map_err(store_failed)?;
            changed += 1;
        }
    }
    let remaining = ledger.model_calls_without_cost().map_err(store_failed)?.len();
    Ok(json!({
        "calls": calls.len(),
        "priced": calls.len() - unpriced.len(),
        "changed": changed,
        "priced_micros": priced_micros,
        "currency": list.currency,
        "by_rule": by_rule,
        "unpriced": unpriced.len(),
        "unpriced_calls": unpriced,
        "remaining_without_cost": remaining,
    }))
}

impl Action for PriceEveryCallAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: Spec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let Some(ledger) = &self.ledger else {
            return Err(ActionError::new(
                "no_store",
                "pricing the calls needs the store they are written in, and this run has none",
            ));
        };
        let list = match &spec.price_list {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|error| {
                    ActionError::new("invalid_input", format!("{path}: {error}"))
                })?;
                PriceList::parse(&text).map_err(|error| ActionError::new("invalid_input", error))?
            }
            None => crate::current_price_list(),
        };
        reprice_every_call(ledger, &list).map(ActionOutcome::Went)
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        declared
            .as_object()
            .map(|fields| {
                fields
                    .keys()
                    .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> RedoEvidence {
        RedoEvidence::SameOperation(PRICE_EVERY_CALL_ACTION.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::EngineIdentity;

    const LIST: &str = r#"{
      "currency": "USD",
      "models": [
        {"id": "model-b", "input_per_million": 2.0, "output_per_million": 20.0, "cached_per_million": 0.2},
        {"id": "model-c", "input_per_million": 5.0, "output_per_million": 50.0}
      ],
      "engines": {
        "engine-a": {"assumed_model": "model-b", "unread_call_equivalent_micros": 7000},
        "local-a": {"per_wall_second_micros": 14},
        "engine-holds": {"assumed_model": "model-b", "input_holds_cached": true}
      },
      "not_models": ["hammer"]
    }"#;

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-price-every-call-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        path
    }

    fn call(id: &str, cli: &str) -> ModelCallRecord {
        ModelCallRecord {
            call_id: id.to_owned(),
            run_id: "run-1".to_owned(),
            step_id: Some("ask".to_owned()),
            purpose: "external_engine".to_owned(),
            cli: cli.to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: EngineIdentity::default(),
            retry_chain: Vec::new(),
            error_type: None,
            started_at: 100,
            ended_at: Some(160),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
            session_mode: None,
            role: None,
            role_resolved_to: Vec::new(),
        }
    }

    fn a_ledger_of_every_shape(dir: &std::path::Path) -> Ledger {
        let ledger = Ledger::open(dir.join("ledger")).expect("a store of the test's own");
        let mut tokens_no_model = call("tokens-no-model", "engine-a");
        tokens_no_model.input_tokens = Some(1_000_000);
        tokens_no_model.output_tokens = Some(0);
        let unread = call("unread", "engine-a");
        let local = call("local", "local-a");
        let tool = call("tool", "hammer");
        let mut declared = call("declared", "engine-z");
        declared.declared_cost_micros = Some(9);
        let mut priced_already = call("priced", "engine-z");
        priced_already.cost_micros = Some(3);
        for record in [&tokens_no_model, &unread, &local, &tool, &declared, &priced_already] {
            ledger.record_model_call(record).unwrap();
        }
        ledger
    }

    #[test]
    fn every_call_a_rule_fits_is_priced_and_the_rule_is_counted() {
        let dir = scratch("all-priced");
        let ledger = a_ledger_of_every_shape(&dir);
        assert_eq!(ledger.model_calls_without_cost().unwrap().len(), 4);

        let said = price_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();
        assert_eq!(said["calls"], 4);
        assert_eq!(said["priced"], 4);
        assert_eq!(said["unpriced"], 0);
        assert_eq!(said["remaining_without_cost"], 0);
        assert_eq!(said["by_rule"]["assumed_model"], 1);
        assert_eq!(said["by_rule"]["unread_call_equivalent"], 1);
        assert_eq!(said["by_rule"]["wall_seconds"], 1);
        assert_eq!(said["by_rule"]["not_a_model_call"], 1);
        assert_eq!(said["priced_micros"], 2_000_000 + 7000 + 60 * 14);

        let rows = ledger
            .browse("SELECT call_id, cost_micros, input_price_micros_per_million, price_currency FROM model_calls ORDER BY call_id", 10)
            .unwrap();
        let by_id: BTreeMap<String, Vec<Value>> = rows
            .rows
            .into_iter()
            .map(|row| (row[0].as_str().unwrap().to_owned(), row))
            .collect();
        assert_eq!(by_id["tokens-no-model"][1], 2_000_000);
        assert_eq!(by_id["tokens-no-model"][2], 2_000_000);
        assert_eq!(by_id["tokens-no-model"][3], "USD");
        assert_eq!(by_id["local"][1], 840);
        assert_eq!(by_id["tool"][1], 0);
        assert_eq!(by_id["declared"][1], Value::Null, "the engine's own figure is left alone");
        assert_eq!(by_id["priced"][1], 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The step asked for a model and the engine named none: the model asked
    /// for is the one that served, not the engine's usual one.
    #[test]
    fn a_model_the_step_asked_for_prices_a_call_whose_engine_named_none() {
        let dir = scratch("asked-for");
        let ledger = Ledger::open(dir.join("ledger")).expect("a store of the test's own");
        let mut asked = call("asked", "engine-a");
        asked.requested_model = "model-c".to_owned();
        asked.input_tokens = Some(1_000_000);
        asked.output_tokens = Some(0);
        ledger.record_model_call(&asked).unwrap();

        let said = price_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();

        assert_eq!(said["priced_micros"], 5_000_000);
        assert_eq!(said["by_rule"]["requested_model"], 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A row priced by an older rule book is priced again by today's, and the
    /// engine's own figure is still left alone.
    #[test]
    fn repricing_corrects_a_figure_an_older_rule_book_got_wrong() {
        let dir = scratch("again");
        let ledger = Ledger::open(dir.join("ledger")).expect("a store of the test's own");
        let mut doubled = call("doubled", "engine-holds");
        doubled.input_tokens = Some(1_000_000);
        doubled.cached_tokens = Some(500_000);
        doubled.output_tokens = Some(0);
        doubled.cost_micros = Some(2_100_000);
        let mut declared = call("declared", "engine-holds");
        declared.input_tokens = Some(1_000_000);
        declared.declared_cost_micros = Some(9);
        declared.cost_micros = Some(1);
        for record in [&doubled, &declared] {
            ledger.record_model_call(record).unwrap();
        }

        let said = reprice_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();

        assert_eq!(said["changed"], 1);
        let rows = ledger
            .browse("SELECT call_id, cost_micros FROM model_calls ORDER BY call_id", 10)
            .unwrap();
        // 500,000 uncached at 2.00 and 500,000 cached at 0.20.
        assert_eq!(rows.rows[1][1], 1_100_000);
        assert_eq!(rows.rows[0][1], 1, "a call the engine declared keeps its row");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A row written before the model asked for was recorded takes it from the
    /// flow definition its run recorded, and keeps it.
    #[test]
    fn a_model_asked_for_is_recovered_from_the_definition_the_run_recorded() {
        let dir = scratch("recovered");
        let ledger = Ledger::open(dir.join("ledger")).expect("a store of the test's own");
        let definition = json!({
            "id": "a-flow",
            "graph": {"steps": [
                {"id": "ask", "action": "external_engine", "with": {"model": {"engine-a": "model-c"}}}
            ]}
        });
        ledger
            .record_flow_definition(&ledger::FlowDefinitionRecord::of("run-1", &definition, 1))
            .unwrap();
        let mut old = call("old", "engine-a");
        old.input_tokens = Some(1_000_000);
        old.output_tokens = Some(0);
        ledger.record_model_call(&old).unwrap();

        let said = reprice_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();

        assert_eq!(said["priced_micros"], 5_000_000);
        let rows = ledger
            .browse("SELECT requested_model FROM model_calls WHERE call_id = 'old'", 1)
            .unwrap();
        assert_eq!(rows.rows[0][0], "model-c");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_recorded_definitions_that_disagree_recover_no_model() {
        let dir = scratch("disagree");
        let ledger = Ledger::open(dir.join("ledger")).expect("a store of the test's own");
        for (model, at) in [("model-c", 1), ("model-b", 2)] {
            let definition = json!({
                "id": "a-flow",
                "graph": {"steps": [
                    {"id": "ask", "action": "external_engine", "with": {"model": {"engine-a": model}}}
                ]}
            });
            ledger
                .record_flow_definition(&ledger::FlowDefinitionRecord::of("run-1", &definition, at))
                .unwrap();
        }
        let mut old = call("old", "engine-a");
        old.input_tokens = Some(1_000_000);
        old.output_tokens = Some(0);
        ledger.record_model_call(&old).unwrap();

        let said = reprice_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();

        assert_eq!(said["by_rule"]["assumed_model"], 1, "no model is recovered from a disagreement");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_call_no_rule_fits_is_named_and_counted_as_unpriced() {
        let dir = scratch("one-unpriced");
        let ledger = a_ledger_of_every_shape(&dir);
        let mut stranger = call("stranger", "engine-nobody-priced");
        stranger.input_tokens = Some(10);
        ledger.record_model_call(&stranger).unwrap();

        let said = price_every_call(&ledger, &PriceList::parse(LIST).unwrap()).unwrap();
        assert_eq!(said["priced"], 4);
        assert_eq!(said["unpriced"], 1);
        assert_eq!(said["remaining_without_cost"], 1);
        assert_eq!(said["unpriced_calls"][0]["call_id"], "stranger");
        assert_eq!(said["unpriced_calls"][0]["cli"], "engine-nobody-priced");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_action_reads_the_list_it_is_pointed_at_and_refuses_a_field_it_does_not_know() {
        let dir = scratch("as-an-action");
        let ledger = a_ledger_of_every_shape(&dir);
        let mut doubled = call("doubled", "engine-holds");
        doubled.input_tokens = Some(1_000_000);
        doubled.cached_tokens = Some(500_000);
        doubled.cost_micros = Some(2_100_000);
        ledger.record_model_call(&doubled).unwrap();
        let list = dir.join("pricing.json");
        std::fs::write(&list, LIST).unwrap();
        let action = PriceEveryCallAction {
            ledger: Some(ledger),
        };
        let outcome = action
            .execute(
                &json!({"price_list": list.display().to_string()}),
                &SharedState::new(),
            )
            .unwrap();
        let ActionOutcome::Went(said) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(said["remaining_without_cost"], 0);
        assert_eq!(said["unpriced"], 0, "a figure no rule can redo is kept, not reported");
        assert_eq!(said["changed"], 5, "four rows without a cost and one priced by an older list");
        assert_eq!(
            action.unknown_fields(&json!({"price_list": "x", "pricelist": "y"})),
            vec!["pricelist"]
        );
        let none = PriceEveryCallAction { ledger: None };
        assert_eq!(
            none.execute(&json!({}), &SharedState::new())
                .unwrap_err()
                .class,
            "no_store"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
