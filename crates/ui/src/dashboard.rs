//! The counts the window shows: from ledger records to the structures the API
//! serializes. Pure — it reads neither disk nor network — so tests can break
//! the arithmetic without touching a real `Ledger`.

use flow::{Outcome, StepRecord};
use ledger::{EngineIdentity, ModelCallRecord, RunRecord};
use serde::Serialize;
use std::collections::BTreeMap;

/// The bucket for calls to an engine that never said which model it used. It
/// is not a model: it is the missing declaration, said in the reader's language.
pub fn model_not_declared() -> String {
    catalogue::say("ui.cost.model_not_declared", &[])
}

/// A micro-unit figure as the catalogue's sentences carry it: in units, to
/// four places, so the two languages format the number the same way.
fn units_of(micros: i64) -> String {
    format!("{:.4}", micros as f64 / 1_000_000.0)
}

/// The counts of a set of calls.
///
/// **ONLY WHAT IS KNOWN GETS SUMMED, AND HOW MUCH IS UNKNOWN GETS SAID.** An
/// unknown count added as zero disappears and the total presents itself as
/// complete while it is partial — exactly the lie this work was born from. So
/// two figures qualify the totals: calls with no tokens, and calls with no cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    /// Tokens **written** to cache, both durations summed.
    ///
    /// **KEPT APART FROM `cached_tokens` BECAUSE THEY ARE OPPOSITES.** Reading
    /// from cache costs a fraction of input; writing to it costs more than
    /// input. On one measured call this entry alone was 96% of the spend, so
    /// putting it in the same box as reads makes the dear part look cheap.
    pub cache_write_tokens: u64,
    /// The total declared by engines that never split the two sides (Codex and
    /// similar). Kept apart because adding it to input and output would count
    /// twice over the engines that report all three numbers.
    pub total_tokens_only: u64,
    pub cost_micros: i64,
    pub calls: usize,
    /// **HOW MANY TURNS IN ALL.** A turn is one lap of the model inside a
    /// single call, and it is the quantity that explains why a chain of steps
    /// costs more than a single session: per turn it reads 8% more, but it
    /// takes twice as many turns. Whoever reads a token total without knowing
    /// across how many turns it was spent cannot tell where to act.
    pub turns: u64,
    /// How many of these calls reported no counts at all.
    pub calls_without_tokens: usize,
    /// How many have no cost: the model was missing from the price list, or the
    /// list carried no price for it.
    pub calls_without_cost: usize,
}

impl TokenTotals {
    fn add(&mut self, call: &ModelCallRecord) {
        self.input_tokens += call.input_tokens.unwrap_or(0);
        self.output_tokens += call.output_tokens.unwrap_or(0);
        self.cached_tokens += call.cached_tokens.unwrap_or(0);
        self.cache_write_tokens +=
            call.cache_write_tokens.unwrap_or(0) + call.cache_write_long_tokens.unwrap_or(0);
        self.turns += call.turns.unwrap_or(0);
        // Only for engines that never reported the sides: the rest are counted above.
        if call.input_tokens.is_none() && call.output_tokens.is_none() {
            self.total_tokens_only += call.total_tokens.unwrap_or(0);
        }
        self.cost_micros += call.cost_micros.unwrap_or(0);
        self.calls += 1;
        if call.input_tokens.is_none()
            && call.output_tokens.is_none()
            && call.cached_tokens.is_none()
            && call.total_tokens.is_none()
        {
            self.calls_without_tokens += 1;
        }
        if call.cost_micros.is_none() {
            self.calls_without_cost += 1;
        }
    }

    /// True when these totals hide something: whoever shows them must say so.
    /// A partial total that declares itself partial is a measurement; a partial
    /// total that stays silent is worse than not having one at all.
    pub fn is_partial(&self) -> bool {
        self.calls_without_tokens > 0 || self.calls_without_cost > 0
    }

    /// How `cost_micros` reads: the total, a floor, or nothing.
    ///
    /// **GOES THROUGH `Spend` RATHER THAN RESTATING THE RULE.** The executor
    /// draws the line between a total and a floor, and stops a run at the cap
    /// on it; a second copy here would agree until it drifted, and the one that
    /// drifts is the one a person reads, which no type checks.
    pub fn cost_reading(&self) -> flow::CostReading {
        flow::Spend {
            micros: self.cost_micros,
            calls: self.calls as i64,
            calls_without_cost: self.calls_without_cost as i64,
            // Useless for reading a total: it says how far whoever opens
            // several steps at once may overshoot, which is a question for
            // whoever decides, never for whoever looks.
            dearest_micros: None,
        }
        .reading()
    }
}

/// The cost figure as it must be written to a person, on one line. **THE
/// QUALIFIER BELONGS INSIDE THE LINE OF THE NUMBER, AND THAT IS THE WHOLE
/// WORK.** A note «partial: 3 calls with no known cost» already existed and was
/// true: it sat two lines lower, and the A/B run was read as 1.6674 dollars
/// when it had cost 7.2080. A note below a number does not correct the number —
/// it accompanies it, and the reader keeps the number.
pub fn how_the_cost_reads(reading: &flow::CostReading) -> String {
    // **WHY THE FLOOR AND NOT THE LIST OF ENTRIES.** Both roads told the truth.
    // A per-call list, though, does not answer the question this command is
    // read for — «can I launch another?» — and leaves the reader to do it in
    // their head, adding up the entries they can see and landing back on the
    // wrong total. A floor answers: *this much has already gone, and it is not
    // all of it*.
    match reading {
        flow::CostReading::Nothing => catalogue::say("ui.cost.nothing", &[]),
        flow::CostReading::Exact(micros) => {
            catalogue::say("ui.cost.exact", &[("units", &units_of(*micros))])
        }
        // Without even one measurement there is no floor to declare: «at least
        // 0.0000» is true, says nothing, and reads as a small outlay. It is the
        // third case of `Spend`, carried all the way through.
        flow::CostReading::AtLeast {
            known_micros: 0,
            calls,
            ..
        } => catalogue::say("ui.cost.unknown", &[("calls", &calls.to_string())]),
        flow::CostReading::AtLeast {
            known_micros,
            calls,
            calls_without_cost,
        } => catalogue::say(
            "ui.cost.at_least",
            &[
                ("units", &units_of(*known_micros)),
                ("calls", &calls.to_string()),
                ("calls_without_cost", &calls_without_cost.to_string()),
            ],
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// A single call as the window sees it. Optional fields leave as `null` in the
/// JSON and the window draws a dash there: never a `0`, which a reader would
/// mistake for a measurement.
pub struct CallView {
    pub call_id: String,
    pub step_id: Option<String>,
    pub purpose: String,
    pub cli: String,
    pub requested_model: String,
    pub actual_model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    /// Written to cache: the dearest entry of all, kept visible.
    pub cache_write_tokens: Option<u64>,
    pub cache_write_long_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// The turns of this call: how many times the model came back to speak
    /// inside a single invocation.
    pub turns: Option<u64>,
    pub cost_micros: Option<i64>,
    /// What the engine declared on its own, beside the price-list figure: when
    /// the two diverge, the divergence shows.
    pub declared_cost_micros: Option<i64>,
    pub error_type: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// Which identity the process behind this call started under.
    ///
    /// **IT TRAVELS THIS FAR SO THAT SOMEBODY LOOKS AT IT.** The value used to
    /// be written to the ledger, read back into a struct, and reach no screen
    /// and no command: data gathered and never looked at is one step away from
    /// becoming wrong data that nobody notices.
    pub engine_identity: EngineIdentity,
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
    /// Where it was born: `None` outside every workspace, or before v10.
    pub worktree: Option<String>,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_secs: Option<i64>,
    pub total_cost_micros: i64,
    pub error: Option<String>,
    pub steps_total: usize,
    pub steps_went: usize,
    pub steps_broke: usize,
    pub steps_retried: usize,
    pub steps_open: Vec<OpenStep>,
    pub tokens: TokenTotals,
    pub tokens_by_model: BTreeMap<String, TokenTotals>,
    pub calls: Vec<CallView>,
}

/// Reduces the history of a run — steps and calls — to a single view. A step
/// counts for its latest attempt alone: a failure followed by a success must
/// never count as two steps.
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
    let mut steps_retried = 0;
    let mut steps_open = Vec::new();
    for step in latest_by_step.values() {
        if step.attempt > 1 {
            steps_retried += 1;
        }
        match step.outcome {
            Some(Outcome::Went) => steps_went += 1,
            Some(Outcome::Broke) => steps_broke += 1,
            None => steps_open.push(OpenStep {
                step_id: step.step_id.clone(),
                attempt: step.attempt,
                started_at: step.started_at,
                open_for_secs: now - step.started_at,
            }),
            // `NotYet` is deliberately not counted as broken: an ordinary poll
            // is not a fault, and putting it in that tally would make a patrol
            // flow read as failing every time it looked.
            Some(Outcome::Waiting)
            | Some(Outcome::NotYet)
            | Some(Outcome::Stopped)
            | Some(Outcome::Skipped) => {}
        }
    }
    steps_open.sort_by(|a, b| a.step_id.cmp(&b.step_id));

    let mut tokens = TokenTotals::default();
    let mut tokens_by_model: BTreeMap<String, TokenTotals> = BTreeMap::new();
    let mut calls_view: Vec<CallView> = calls
        .iter()
        .map(|call| {
            tokens.add(call);
            // A command-line engine does not always name the model that served
            // the call. Grouping those rows under an empty key would make them
            // invisible in the per-model list; here they carry a name that says
            // what they are.
            let by_model = if call.actual_model.trim().is_empty() {
                model_not_declared()
            } else {
                call.actual_model.clone()
            };
            tokens_by_model.entry(by_model).or_default().add(call);
            CallView {
                call_id: call.call_id.clone(),
                step_id: call.step_id.clone(),
                purpose: call.purpose.clone(),
                cli: call.cli.clone(),
                requested_model: call.requested_model.clone(),
                actual_model: call.actual_model.clone(),
                input_tokens: call.input_tokens,
                output_tokens: call.output_tokens,
                cached_tokens: call.cached_tokens,
                cache_write_tokens: call.cache_write_tokens,
                cache_write_long_tokens: call.cache_write_long_tokens,
                total_tokens: call.total_tokens,
                turns: call.turns,
                cost_micros: call.cost_micros,
                declared_cost_micros: call.declared_cost_micros,
                error_type: call.error_type.clone(),
                started_at: call.started_at,
                ended_at: call.ended_at,
                engine_identity: call.engine_identity.clone(),
            }
        })
        .collect();
    calls_view.sort_by_key(|call| call.started_at);

    ExecutionView {
        run_id: run.run_id.clone(),
        kind: run.kind.clone(),
        entity: run.entity.clone(),
        worktree: run.worktree.clone(),
        status: run.status.clone(),
        started_at: run.started_at,
        ended_at: run.ended_at,
        duration_secs: run.ended_at.map(|ended| ended - run.started_at),
        total_cost_micros: run.total_cost_micros,
        error: run.error.clone(),
        steps_total: latest_by_step.len(),
        steps_went,
        steps_broke,
        steps_retried,
        steps_open,
        tokens,
        tokens_by_model,
        calls: calls_view,
    }
}

/// Which identities a run's calls started under, and how many each, **in the
/// order they appeared**.
///
/// **WHY A LIST AND NOT ONE.** A run can change identity halfway: a step that
/// writes its own home, a fallback engine that is not the known one, a step
/// delegated to an agent. Showing the first makes a mixed run look uniform.
pub fn identities_of(calls: &[CallView]) -> Vec<(EngineIdentity, usize)> {
    // Order of appearance and not of count: whoever reads a report is
    // reconstructing what happened, and the chronology is the thread they
    // follow. `EngineIdentity` is not orderable — and must not become so just
    // to fit a datum inside a map.
    let mut seen: Vec<(EngineIdentity, usize)> = Vec::new();
    for call in calls {
        match seen
            .iter_mut()
            .find(|(identity, _)| identity == &call.engine_identity)
        {
            Some((_, how_many)) => *how_many += 1,
            None => seen.push((call.engine_identity.clone(), 1)),
        }
    }
    seen
}

/// Builds the full history, most recent at the top.
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
    executions.sort_by_key(|execution| std::cmp::Reverse(execution.started_at));
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
            started_by: "test".to_owned(),
            status: "running".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at,
            ended_at,
            worktree: None,
            stop_reason: None,
        }
    }

    fn step(
        step_id: &str,
        attempt: u32,
        epoch: u64,
        outcome: Option<Outcome>,
        started_at: i64,
    ) -> StepRecord {
        let mut record = StepRecord::started(
            "run-1",
            step_id,
            attempt,
            epoch,
            vec![],
            json!({}),
            vec![],
            started_at,
        );
        record.outcome = outcome;
        record.ended_at = outcome.map(|_| started_at + 1);
        record
    }

    fn call(
        actual_model: &str,
        input: u64,
        output: u64,
        cached: u64,
        cost: i64,
    ) -> ModelCallRecord {
        measured(
            actual_model,
            Some(input),
            Some(output),
            Some(cached),
            Some(cost),
        )
    }

    /// A call with whatever counts you want, «unknown» included.
    pub(super) fn measured(
        actual_model: &str,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost: Option<i64>,
    ) -> ModelCallRecord {
        ModelCallRecord {
            call_id: format!("call-{actual_model}-{input:?}-{cost:?}"),
            run_id: "run-1".to_owned(),
            step_id: None,
            purpose: "test".to_owned(),
            cli: "claude".to_owned(),
            requested_model: actual_model.to_owned(),
            actual_model: actual_model.to_owned(),
            input_tokens: input,
            output_tokens: output,
            cached_tokens: cached,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: cost,
            declared_cost_micros: None,
            price_currency: Some("USD".to_owned()),
            input_price_micros_per_million: Some(0),
            output_price_micros_per_million: Some(0),
            cached_price_micros_per_million: Some(0),
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: EngineIdentity::default(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: None,
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
            session_mode: None,
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
        assert_eq!(view.steps_retried, 1);
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

    /// **THE DEAREST ENTRY MUST NOT VANISH FROM THE TOTALS.** The numbers come
    /// from a real run: 2 input tokens, 4 output, 13,180 read from cache and
    /// 8,961 written. Were the writes left out of the totals, the view would
    /// show a fifteen-thousand-token run costing ninety-six thousandths of a
    /// dollar as though the money came from somewhere else — and a total nobody
    /// can redo by hand is a total nobody can dispute.
    #[test]
    fn the_cache_that_was_written_is_counted_and_kept_apart_from_the_one_read() {
        let mut written = call("claude-opus-5[1m]", 2, 4, 13_180, 96_310);
        written.cache_write_long_tokens = Some(8_961);
        let view = summarize_run(&run("run-1", 0, None), &[], &[written], 0);

        assert_eq!(
            view.tokens.cache_write_tokens, 8_961,
            "what was written to cache counts"
        );
        assert_eq!(
            view.tokens.cached_tokens, 13_180,
            "and stays apart from what was read: they are opposites"
        );
        assert_eq!(view.tokens.cost_micros, 96_310);
        assert!(
            !view.tokens.is_partial(),
            "this call declared everything: the total is complete"
        );
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

#[cfg(test)]
mod what_is_not_known {
    //! The counts when a call never said how much it consumed.

    use super::tests::measured;
    use super::*;
    use serde_json::json;

    fn run() -> RunRecord {
        RunRecord {
            run_id: "run-1".to_owned(),
            kind: "test".to_owned(),
            entity: "test".to_owned(),
            parent_run_id: None,
            started_by: "test".to_owned(),
            status: "succeeded".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
            worktree: None,
            stop_reason: None,
        }
    }

    /// **THE HONESTY CONSTRAINT, IN ONE NUMBER.** An unmeasured call never
    /// enters the sum as zero. Were it to, these two totals would be identical
    /// and whoever looked would believe they held a complete measurement where
    /// they hold half of one.
    #[test]
    fn an_unmeasured_call_is_not_summed_as_zero_and_is_counted_apart() {
        let known = measured("m", Some(100), Some(50), Some(10), Some(500));
        let unknown = measured("m", None, None, None, None);

        let only_known = summarize_run(&run(), &[], std::slice::from_ref(&known), 20);
        let with_unknown = summarize_run(&run(), &[], &[known, unknown], 20);

        // The summed tokens are the same: the unknown one added no zeros.
        assert_eq!(
            with_unknown.tokens.input_tokens,
            only_known.tokens.input_tokens
        );
        assert_eq!(
            with_unknown.tokens.cost_micros,
            only_known.tokens.cost_micros
        );
        // But the total knows it is partial, and says by how much.
        assert_eq!(with_unknown.tokens.calls, 2);
        assert_eq!(with_unknown.tokens.calls_without_tokens, 1);
        assert_eq!(with_unknown.tokens.calls_without_cost, 1);
        assert!(with_unknown.tokens.is_partial());
        assert!(
            !only_known.tokens.is_partial(),
            "a complete total must never call itself partial, or the warning stops meaning anything"
        );
    }

    /// In the JSON the window receives, an unknown is `null`. A `0` would be
    /// indistinguishable from a measurement, and the window would lose any way
    /// to draw a dash there.
    #[test]
    fn an_unknown_count_leaves_the_api_as_null_never_as_zero() {
        let view = summarize_run(&run(), &[], &[measured("m", None, None, None, None)], 20);
        let payload = json!(view);
        let call = &payload["calls"][0];
        assert_eq!(call["input_tokens"], json!(null));
        assert_eq!(call["output_tokens"], json!(null));
        assert_eq!(call["cached_tokens"], json!(null));
        assert_eq!(call["cost_micros"], json!(null));
    }

    /// An engine that declares the total alone — codex and similar — keeps its
    /// one true measurement, and never sees it added to sides that do not exist.
    #[test]
    fn a_total_only_engine_keeps_its_one_true_measure_apart() {
        let mut total_only = measured("m", None, None, None, None);
        total_only.total_tokens = Some(13_910);
        let view = summarize_run(&run(), &[], &[total_only], 20);
        assert_eq!(view.tokens.input_tokens, 0, "it has no sides to sum");
        assert_eq!(view.tokens.total_tokens_only, 13_910);
        assert_eq!(
            view.tokens.calls_without_tokens, 0,
            "a declared total is a measurement: this call is never among the silent ones"
        );
    }

    /// An engine that never names the model lands under a name that says what
    /// it is, rather than an empty key that vanishes from the per-model list.
    #[test]
    fn calls_without_a_declared_model_are_grouped_under_a_name_that_says_so() {
        let view = summarize_run(
            &run(),
            &[],
            &[measured("", Some(1), Some(1), None, None)],
            20,
        );
        assert!(view.tokens_by_model.contains_key(&model_not_declared()));
        assert!(!view.tokens_by_model.contains_key(""));
        assert_eq!(
            model_not_declared(),
            catalogue::say("ui.cost.model_not_declared", &[]),
            "the bucket's name is the catalogue's, not a word written into the code"
        );
    }

    /// Each of the four readings is the catalogue's sentence with the same
    /// values, not a line of its own: a line written into the code is English
    /// for everyone, and nothing else in this crate would have said so.
    #[test]
    fn the_cost_line_is_the_catalogues_sentence_in_every_case() {
        assert_eq!(
            how_the_cost_reads(&flow::CostReading::Nothing),
            catalogue::say("ui.cost.nothing", &[])
        );
        assert_eq!(
            how_the_cost_reads(&flow::CostReading::Exact(1_667_400)),
            catalogue::say("ui.cost.exact", &[("units", "1.6674")])
        );
        assert_eq!(
            how_the_cost_reads(&flow::CostReading::AtLeast {
                known_micros: 0,
                calls: 3,
                calls_without_cost: 3,
            }),
            catalogue::say("ui.cost.unknown", &[("calls", "3")])
        );
        assert_eq!(
            how_the_cost_reads(&flow::CostReading::AtLeast {
                known_micros: 1_667_400,
                calls: 4,
                calls_without_cost: 3,
            }),
            catalogue::say(
                "ui.cost.at_least",
                &[
                    ("units", "1.6674"),
                    ("calls", "4"),
                    ("calls_without_cost", "3"),
                ]
            )
        );
    }
}
