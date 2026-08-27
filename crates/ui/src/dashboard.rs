//! I conti che la pagina mostra: dai record del deposito alle strutture
//! serializzate per la API. Pura — nessuna lettura di disco o di rete —
//! così le prove rompono i conti senza toccare un `Ledger` vero.

use flow::{Outcome, StepRecord};
use ledger::{ModelCallRecord, RunRecord};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_micros: i64,
    pub calls: usize,
}

impl TokenTotals {
    fn add(&mut self, call: &ModelCallRecord) {
        self.input_tokens += call.input_tokens;
        self.output_tokens += call.output_tokens;
        self.cached_tokens += call.cached_tokens;
        self.cost_micros += call.cost_micros;
        self.calls += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CallView {
    pub call_id: String,
    pub step_id: Option<String>,
    pub purpose: String,
    pub requested_model: String,
    pub actual_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_micros: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenStep {
    pub step_id: String,
    pub attempt: u32,
    pub started_at: i64,
    pub open_for_secs: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExecutionView {
    pub run_id: String,
    pub kind: String,
    pub entity: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_secs: Option<i64>,
    pub total_cost_micros: i64,
    pub error: Option<String>,
    pub steps_total: usize,
    pub steps_went: usize,
    pub steps_broke: usize,
    pub steps_open: Vec<OpenStep>,
    pub tokens: TokenTotals,
    pub tokens_by_model: BTreeMap<String, TokenTotals>,
    pub calls: Vec<CallView>,
}

/// Riduce la storia di un'esecuzione — passi e chiamate — a un'unica vista.
/// Un passo conta per il suo ultimo tentativo soltanto: un fallimento seguito
/// da un successo non deve contare come due passi.
pub fn summarize_run(
    run: &RunRecord,
    steps: &[StepRecord],
    calls: &[ModelCallRecord],
    now: i64,
) -> ExecutionView {
    let mut latest_by_step: BTreeMap<&str, &StepRecord> = BTreeMap::new();
    for step in steps {
        latest_by_step
            .entry(step.step_id.as_str())
            .and_modify(|current| {
                if (step.attempt, step.epoch) > (current.attempt, current.epoch) {
                    *current = step;
                }
            })
            .or_insert(step);
    }

    let mut steps_went = 0;
    let mut steps_broke = 0;
    let mut steps_open = Vec::new();
    for step in latest_by_step.values() {
        match step.outcome {
            Some(Outcome::Went) => steps_went += 1,
            Some(Outcome::Broke) => steps_broke += 1,
            None => steps_open.push(OpenStep {
                step_id: step.step_id.clone(),
                attempt: step.attempt,
                started_at: step.started_at,
                open_for_secs: now - step.started_at,
            }),
            Some(Outcome::Waiting) | Some(Outcome::Stopped) | Some(Outcome::Skipped) => {}
        }
    }
    steps_open.sort_by(|a, b| a.step_id.cmp(&b.step_id));

    let mut tokens = TokenTotals::default();
    let mut tokens_by_model: BTreeMap<String, TokenTotals> = BTreeMap::new();
    let mut calls_view: Vec<CallView> = calls
        .iter()
        .map(|call| {
            tokens.add(call);
            tokens_by_model
                .entry(call.actual_model.clone())
                .or_default()
                .add(call);
            CallView {
                call_id: call.call_id.clone(),
                step_id: call.step_id.clone(),
                purpose: call.purpose.clone(),
                requested_model: call.requested_model.clone(),
                actual_model: call.actual_model.clone(),
                input_tokens: call.input_tokens,
                output_tokens: call.output_tokens,
                cached_tokens: call.cached_tokens,
                cost_micros: call.cost_micros,
                started_at: call.started_at,
                ended_at: call.ended_at,
            }
        })
        .collect();
    calls_view.sort_by_key(|call| call.started_at);

    ExecutionView {
        run_id: run.run_id.clone(),
        kind: run.kind.clone(),
        entity: run.entity.clone(),
        status: run.status.clone(),
        started_at: run.started_at,
        ended_at: run.ended_at,
        duration_secs: run.ended_at.map(|ended| ended - run.started_at),
        total_cost_micros: run.total_cost_micros,
        error: run.error.clone(),
        steps_total: latest_by_step.len(),
        steps_went,
        steps_broke,
        steps_open,
        tokens,
        tokens_by_model,
        calls: calls_view,
    }
}

/// Costruisce la storia completa, più recenti in cima.
pub fn build_executions(
    runs: &[RunRecord],
    steps_by_run: &BTreeMap<String, Vec<StepRecord>>,
    calls_by_run: &BTreeMap<String, Vec<ModelCallRecord>>,
    now: i64,
) -> Vec<ExecutionView> {
    let empty_steps: Vec<StepRecord> = Vec::new();
    let empty_calls: Vec<ModelCallRecord> = Vec::new();
    let mut executions: Vec<ExecutionView> = runs
        .iter()
        .map(|run| {
            let steps = steps_by_run.get(&run.run_id).unwrap_or(&empty_steps);
            let calls = calls_by_run.get(&run.run_id).unwrap_or(&empty_calls);
            summarize_run(run, steps, calls, now)
        })
        .collect();
    executions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    executions
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(run_id: &str, started_at: i64, ended_at: Option<i64>) -> RunRecord {
        RunRecord {
            run_id: run_id.to_owned(),
            kind: "sweep".to_owned(),
            entity: "marker-sweep".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "running".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at,
            ended_at,
        }
    }

    fn step(
        step_id: &str,
        attempt: u32,
        epoch: u64,
        outcome: Option<Outcome>,
        started_at: i64,
    ) -> StepRecord {
        let mut record =
            StepRecord::started("run-1", step_id, attempt, epoch, vec![], json!({}), vec![], started_at);
        record.outcome = outcome;
        record.ended_at = outcome.map(|_| started_at + 1);
        record
    }

    fn call(actual_model: &str, input: u64, output: u64, cached: u64, cost: i64) -> ModelCallRecord {
        ModelCallRecord {
            call_id: format!("call-{actual_model}-{input}"),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "prova".to_owned(),
            cli: "claude".to_owned(),
            requested_model: actual_model.to_owned(),
            actual_model: actual_model.to_owned(),
            input_tokens: input,
            output_tokens: output,
            cached_tokens: cached,
            cost_micros: cost,
            price_currency: "USD".to_owned(),
            input_price_micros_per_million: 0,
            output_price_micros_per_million: 0,
            cached_price_micros_per_million: 0,
            mandate_name: "prova".to_owned(),
            mandate_version: "1".to_owned(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: None,
        }
    }

    #[test]
    fn only_the_latest_attempt_of_a_step_is_counted() {
        let steps = vec![
            step("scan", 1, 1, Some(Outcome::Broke), 10),
            step("scan", 2, 2, Some(Outcome::Went), 20),
        ];
        let view = summarize_run(&run("run-1", 0, None), &steps, &[], 100);
        assert_eq!(view.steps_total, 1);
        assert_eq!(view.steps_went, 1);
        assert_eq!(view.steps_broke, 0);
    }

    #[test]
    fn a_step_still_without_an_outcome_is_reported_open_with_its_age() {
        let steps = vec![step("remove", 1, 1, None, 40)];
        let view = summarize_run(&run("run-1", 0, None), &steps, &[], 100);
        assert_eq!(view.steps_open.len(), 1);
        assert_eq!(view.steps_open[0].step_id, "remove");
        assert_eq!(view.steps_open[0].open_for_secs, 60);
    }

    #[test]
    fn a_closed_step_is_never_reported_open() {
        let steps = vec![step("scan", 1, 1, Some(Outcome::Went), 10)];
        let view = summarize_run(&run("run-1", 0, None), &steps, &[], 100);
        assert!(view.steps_open.is_empty());
    }

    #[test]
    fn tokens_are_summed_overall_and_split_by_actual_model() {
        let calls = vec![
            call("model-a", 100, 10, 1, 500),
            call("model-a", 50, 5, 0, 250),
            call("model-b", 200, 20, 2, 900),
        ];
        let view = summarize_run(&run("run-1", 0, None), &[], &calls, 0);
        assert_eq!(view.tokens.input_tokens, 350);
        assert_eq!(view.tokens.cost_micros, 1650);
        assert_eq!(view.tokens.calls, 3);
        assert_eq!(view.tokens_by_model.len(), 2);
        assert_eq!(view.tokens_by_model["model-a"].input_tokens, 150);
        assert_eq!(view.tokens_by_model["model-a"].calls, 2);
        assert_eq!(view.tokens_by_model["model-b"].input_tokens, 200);
    }

    #[test]
    fn duration_is_none_while_the_run_has_not_ended() {
        let view = summarize_run(&run("run-1", 10, None), &[], &[], 100);
        assert_eq!(view.duration_secs, None);
        let ended = summarize_run(&run("run-1", 10, Some(70)), &[], &[], 100);
        assert_eq!(ended.duration_secs, Some(60));
    }

    #[test]
    fn executions_are_sorted_most_recent_first() {
        let runs = vec![run("old", 10, None), run("new", 90, None)];
        let executions = build_executions(&runs, &BTreeMap::new(), &BTreeMap::new(), 100);
        assert_eq!(executions[0].run_id, "new");
        assert_eq!(executions[1].run_id, "old");
    }
}
