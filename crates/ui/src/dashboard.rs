//! I conti che la pagina mostra: dai record del deposito alle strutture
//! serializzate per la API. Pura — nessuna lettura di disco o di rete —
//! così le prove rompono i conti senza toccare un `Ledger` vero.

use flow::{Outcome, StepRecord};
use ledger::{ModelCallRecord, RunRecord};
use serde::Serialize;
use std::collections::BTreeMap;

/// Sotto quale nome si raggruppano le chiamate a un motore che non ha detto
/// quale modello ha usato. Non è un modello: è la dichiarazione che manca.
pub const MODEL_NOT_DECLARED: &str = "(modello non dichiarato)";

/// I conti di un insieme di chiamate.
///
/// **SI SOMMA SOLO CIÒ CHE SI SA, E SI DICE QUANTO NON SI SA.** Un conteggio
/// sconosciuto non entra nella somma come zero: uno zero sommato sparisce, e il
/// totale si presenta come completo mentre è parziale — che è esattamente la
/// bugia da cui questo lavoro nasce. Accanto ai totali stanno quindi le due
/// cifre che li qualificano: quante chiamate non hanno detto i propri token, e
/// quante non hanno un costo. Un totale parziale che dichiara di essere parziale
/// è una misura; un totale parziale che tace è peggio del non averlo.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    /// I token **scritti** in cache, le due durate sommate.
    ///
    /// **STANNO A PARTE DA `cached_tokens` PERCHÉ SONO L'OPPOSTO.** Leggere
    /// dalla cache costa una frazione dell'ingresso; scriverci costa più
    /// dell'ingresso. Su una chiamata misurata il 30/08/2026 questa voce da sola
    /// era il 96% della spesa: metterla nella stessa casella di ciò che si legge
    /// farebbe sembrare economico esattamente ciò che è caro.
    pub cache_write_tokens: u64,
    /// Il totale dichiarato da chi non separa i due lati (Codex e simili).
    /// Sta a parte perché sommarlo a ingresso e uscita conterebbe due volte i
    /// motori che dicono tutti e tre i numeri.
    pub total_tokens_only: u64,
    pub cost_micros: i64,
    pub calls: usize,
    /// Quante di queste chiamate non hanno detto nessun conteggio.
    pub calls_without_tokens: usize,
    /// Quante non hanno un costo: il modello non era nel listino, o il listino
    /// non aveva il suo prezzo.
    pub calls_without_cost: usize,
}

impl TokenTotals {
    fn add(&mut self, call: &ModelCallRecord) {
        self.input_tokens += call.input_tokens.unwrap_or(0);
        self.output_tokens += call.output_tokens.unwrap_or(0);
        self.cached_tokens += call.cached_tokens.unwrap_or(0);
        self.cache_write_tokens +=
            call.cache_write_tokens.unwrap_or(0) + call.cache_write_long_tokens.unwrap_or(0);
        // Solo per chi non ha detto i lati: chi li ha detti è già contato sopra.
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

    /// Vero se questi totali nascondono qualcosa: chi li mostra deve dirlo.
    pub fn is_partial(&self) -> bool {
        self.calls_without_tokens > 0 || self.calls_without_cost > 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Una singola chiamata come la vede la pagina. I campi facoltativi escono
/// `null` nel JSON, e la pagina ci scrive un trattino: mai uno `0`, che chi
/// legge scambierebbe per una misura.
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
    /// Scritti in cache: la voce che costa più di tutte, tenuta visibile.
    pub cache_write_tokens: Option<u64>,
    pub cache_write_long_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_micros: Option<i64>,
    /// Quanto il motore ha dichiarato di suo, accanto al conto del listino:
    /// se i due divergono, la divergenza si vede.
    pub declared_cost_micros: Option<i64>,
    pub error_type: Option<String>,
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
    pub steps_retried: usize,
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
            // Un motore a riga di comando non nomina sempre il modello che ha
            // servito la chiamata. Raggruppare quelle righe sotto una chiave
            // vuota le renderebbe invisibili nell'elenco per modello: qui hanno
            // un nome che dice cosa sono.
            let by_model = if call.actual_model.trim().is_empty() {
                MODEL_NOT_DECLARED.to_owned()
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
                cost_micros: call.cost_micros,
                declared_cost_micros: call.declared_cost_micros,
                error_type: call.error_type.clone(),
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
        steps_retried,
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
        measured(actual_model, Some(input), Some(output), Some(cached), Some(cost))
    }

    /// Una chiamata con i conteggi come li si vuole, compreso «non li so».
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
            purpose: "prova".to_owned(),
            cli: "claude".to_owned(),
            requested_model: actual_model.to_owned(),
            actual_model: actual_model.to_owned(),
            input_tokens: input,
            output_tokens: output,
            cached_tokens: cached,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            cost_micros: cost,
            declared_cost_micros: None,
            price_currency: Some("USD".to_owned()),
            input_price_micros_per_million: Some(0),
            output_price_micros_per_million: Some(0),
            cached_price_micros_per_million: Some(0),
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
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

    /// **LA VOCE PIÙ CARA NON DEVE SPARIRE DAI TOTALI.**
    ///
    /// I numeri sono quelli di una corsa vera del 30/08/2026: 2 token
    /// d'ingresso, 4 d'uscita, 13.180 letti dalla cache e 8.961 scritti. Se la
    /// scrittura non entrasse nei totali, la vista mostrerebbe una corsa da
    /// quindicimila token e novantasei millesimi di dollaro come se il denaro
    /// fosse venuto da qualche altra parte — un totale che non si riesce a
    /// rifare a mano è un totale che nessuno può contestare.
    #[test]
    fn the_cache_that_was_written_is_counted_and_kept_apart_from_the_one_read() {
        let mut written = call("claude-opus-5[1m]", 2, 4, 13_180, 96_310);
        written.cache_write_long_tokens = Some(8_961);
        let view = summarize_run(&run("run-1", 0, None), &[], &[written], 0);

        assert_eq!(view.tokens.cache_write_tokens, 8_961, "la scrittura si conta");
        assert_eq!(
            view.tokens.cached_tokens, 13_180,
            "e resta separata da ciò che si è letto: sono l'opposto l'uno dell'altro"
        );
        assert_eq!(view.tokens.cost_micros, 96_310);
        assert!(
            !view.tokens.is_partial(),
            "questa chiamata ha dichiarato tutto: il totale non è parziale"
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
mod cio_che_non_si_sa {
    //! I conti quando una chiamata non ha detto quanto ha consumato.

    use super::tests::measured;
    use super::*;
    use serde_json::json;

    fn run() -> RunRecord {
        RunRecord {
            run_id: "run-1".to_owned(),
            kind: "prova".to_owned(),
            entity: "prova".to_owned(),
            parent_run_id: None,
            started_by: "prova".to_owned(),
            status: "succeeded".to_owned(),
            total_cost_micros: 0,
            error: None,
            started_at: 0,
            ended_at: Some(10),
        }
    }

    /// **IL VINCOLO SULLA CHIAREZZA PER CHI GUARDA, IN UN NUMERO.** Una
    /// chiamata non misurata non entra nella somma come zero. Se lo facesse,
    /// questi due totali sarebbero identici e chi guarda crederebbe di avere
    /// una misura completa dove ne ha metà.
    #[test]
    fn an_unmeasured_call_is_not_summed_as_zero_and_is_counted_apart() {
        let misurata = measured("m", Some(100), Some(50), Some(10), Some(500));
        let ignota = measured("m", None, None, None, None);

        let solo_nota = summarize_run(&run(), &[], std::slice::from_ref(&misurata), 20);
        let con_ignota = summarize_run(&run(), &[], &[misurata, ignota], 20);

        // I token sommati sono gli stessi: quella ignota non ha aggiunto zeri.
        assert_eq!(con_ignota.tokens.input_tokens, solo_nota.tokens.input_tokens);
        assert_eq!(con_ignota.tokens.cost_micros, solo_nota.tokens.cost_micros);
        // Ma il totale sa di essere parziale, e dice di quanto.
        assert_eq!(con_ignota.tokens.calls, 2);
        assert_eq!(con_ignota.tokens.calls_without_tokens, 1);
        assert_eq!(con_ignota.tokens.calls_without_cost, 1);
        assert!(con_ignota.tokens.is_partial());
        assert!(
            !solo_nota.tokens.is_partial(),
            "un totale completo non deve dichiararsi parziale, o l'avviso perde valore"
        );
    }

    /// Nel JSON che la pagina riceve, uno sconosciuto è `null`. Un `0` sarebbe
    /// indistinguibile da una misura, e la pagina non avrebbe più modo di
    /// scriverci un trattino.
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

    /// Un motore che dichiara solo il totale — codex e simili — non perde la
    /// sua unica misura vera, e non la vede sommata a lati che non esistono.
    #[test]
    fn a_total_only_engine_keeps_its_one_true_measure_apart() {
        let mut solo_totale = measured("m", None, None, None, None);
        solo_totale.total_tokens = Some(13_910);
        let view = summarize_run(&run(), &[], &[solo_totale], 20);
        assert_eq!(view.tokens.input_tokens, 0, "non ha lati da sommare");
        assert_eq!(view.tokens.total_tokens_only, 13_910);
        assert_eq!(
            view.tokens.calls_without_tokens, 0,
            "un totale dichiarato è una misura: questa chiamata non è fra le mute"
        );
    }

    /// Un motore che non nomina il modello finisce sotto un nome che dice cosa
    /// è, non sotto una chiave vuota che nell'elenco per modello sparirebbe.
    #[test]
    fn calls_without_a_declared_model_are_grouped_under_a_name_that_says_so() {
        let view = summarize_run(&run(), &[], &[measured("", Some(1), Some(1), None, None)], 20);
        assert!(view.tokens_by_model.contains_key(MODEL_NOT_DECLARED));
        assert!(!view.tokens_by_model.contains_key(""));
    }
}
