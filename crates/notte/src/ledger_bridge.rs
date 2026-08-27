//! Il secondo osservatore dei cicli di notte: oltre al registro di testo e ai
//! nomi con pid (che restano la verità operativa), ogni lavorazione che
//! arriva a chiamare un motore scrive anche nel deposito durevole di
//! `crates/ledger`. Un guasto qui — deposito non apribile, disco pieno,
//! scrittura fallita — non deve mai fermare una lavorazione: ogni chiamata si
//! inghiotte il proprio errore.
//!
//! PONTE A TERMINE — smontare quando `notte` diventa un flusso vero eseguito
//! da `crates/flow` invece che dal ciclo scritto a mano in `main.rs`: quel
//! giorno il deposito smette di essere un secondo osservatore e diventa la
//! sola verità, e questo file sparisce.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// La chiamata al motore e la sua verifica contano come un solo passo nel
/// deposito: è il ciclo intero di un compito, non le sue fasi interne.
const STEP_ID: &str = "motore";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Il percorso del deposito, da ambiente: niente cablato.
pub fn ledger_dir() -> PathBuf {
    let default = format!(
        "{}/.claude/state/flussi",
        std::env::var("HOME").unwrap_or_default()
    );
    PathBuf::from(std::env::var("NOTTE_LEDGER_DIR").unwrap_or(default))
}

/// L'osservazione di una singola lavorazione, dall'intenzione all'esito.
/// `ledger` è `None` quando il deposito non si è potuto aprire: da lì in poi
/// ogni metodo diventa un no-op, e chi lo usa non se ne accorge.
pub struct LedgerHandle {
    ledger: Option<Ledger>,
    run_id: String,
    task_name: String,
    started_at: i64,
}

impl LedgerHandle {
    /// Apre il deposito e scrive l'intenzione — run e passo — PRIMA che il
    /// chiamante lanci il motore. Se l'apertura o la scrittura falliscono, il
    /// deposito resta muto per tutta la vita di questo handle: non si
    /// ritenta a metà lavorazione.
    pub fn begin(task_name: &str, engine: &str) -> Self {
        Self::begin_in(&ledger_dir(), task_name, engine)
    }

    fn begin_in(dir: &Path, task_name: &str, engine: &str) -> Self {
        let started_at = now_secs();
        let run_id = format!("notte-{task_name}-{started_at}-{}", std::process::id());
        let ledger = Ledger::open(dir).ok();
        if let Some(ledger) = &ledger {
            let _ = ledger.record_run(&RunRecord {
                run_id: run_id.clone(),
                kind: "notte".to_string(),
                entity: task_name.to_string(),
                parent_run_id: None,
                started_by: "notte".to_string(),
                status: "running".to_string(),
                total_cost_micros: 0,
                error: None,
                started_at,
                ended_at: None,
            });
            let record = StepRecord::started(
                run_id.clone(),
                STEP_ID,
                1,
                1,
                Vec::new(),
                json!({ "engine": engine, "task": task_name }),
                Vec::new(),
                started_at,
            );
            let _ = ledger.append_step_started(&record);
        }
        LedgerHandle {
            ledger,
            run_id,
            task_name: task_name.to_string(),
            started_at,
        }
    }

    /// I token, quando si conoscono. `tokens` è il testo grezzo che `notte`
    /// già produce (`"13910"`, `"?"`...): un valore non numerico — cioè
    /// sconosciuto — non scrive nessuna riga, mai uno zero inventato.
    ///
    /// Codex e OpenRouter oggi danno solo un totale, non la divisione fra
    /// ingresso e uscita: finché resta così, il totale va in `output_tokens`
    /// e `input_tokens` resta 0 — un limite del ponte, non un'invenzione.
    pub fn record_tokens(&self, model_label: &str, cli: &str, tokens: &str) {
        let Some(ledger) = &self.ledger else { return };
        let Ok(total) = tokens.trim().parse::<u64>() else {
            return;
        };
        let now = now_secs();
        let call = ModelCallRecord {
            call_id: format!("{}-call", self.run_id),
            run_id: self.run_id.clone(),
            step_id: Some(STEP_ID.to_string()),
            purpose: "notte".to_string(),
            cli: cli.to_string(),
            requested_model: model_label.to_string(),
            actual_model: model_label.to_string(),
            input_tokens: 0,
            output_tokens: total,
            cached_tokens: 0,
            cost_micros: 0,
            price_currency: "USD".to_string(),
            input_price_micros_per_million: 0,
            output_price_micros_per_million: 0,
            cached_price_micros_per_million: 0,
            mandate_name: "notte".to_string(),
            mandate_version: "1".to_string(),
            retry_chain: Vec::new(),
            error_type: None,
            started_at: now,
            ended_at: Some(now),
        };
        let _ = ledger.record_model_call(&call);
    }

    pub fn finish_went(&self) {
        self.close(Outcome::Went, None);
        self.record_run_status("green", None);
    }

    pub fn finish_broke(&self, failure_class: &str, error: &str) {
        self.close(Outcome::Broke, Some(failure_class));
        self.record_run_status(&format!("red ({failure_class})"), Some(error));
    }

    pub fn finish_waiting(&self, reason: &str) {
        self.close(Outcome::Waiting, None);
        self.record_run_status(&format!("rimandato ({reason})"), None);
    }

    pub fn finish_skipped(&self, reason: &str) {
        self.close(Outcome::Skipped, None);
        self.record_run_status(&format!("saltato ({reason})"), None);
    }

    fn close(&self, outcome: Outcome, failure_class: Option<&str>) {
        let Some(ledger) = &self.ledger else { return };
        let completion = Completion {
            outcome,
            output: None,
            said: None,
            failure_class: failure_class.map(|s| s.to_string()),
            ended_at: now_secs(),
            bytes_seen: None,
            bytes_discarded: None,
        };
        let _ = ledger.close_step(&self.run_id, STEP_ID, 1, 1, completion);
    }

    fn record_run_status(&self, status: &str, error: Option<&str>) {
        let Some(ledger) = &self.ledger else { return };
        let _ = ledger.record_run(&RunRecord {
            run_id: self.run_id.clone(),
            kind: "notte".to_string(),
            entity: self.task_name.clone(),
            parent_run_id: None,
            started_by: "notte".to_string(),
            status: status.to_string(),
            total_cost_micros: 0,
            error: error.map(|s| s.to_string()),
            started_at: self.started_at,
            ended_at: Some(now_secs()),
        });
    }

    #[cfg(test)]
    fn is_connected(&self) -> bool {
        self.ledger.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ledger_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "notte-ledger-bridge-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    /// IL BRACCIO CHE CONTA: un deposito che non si può aprire non deve mai
    /// far andare in panico chi lo usa. Punta la cartella su un file già
    /// esistente — `create_dir_all` fallisce di sicuro — e chiama ogni
    /// metodo pubblico.
    #[test]
    fn a_broken_ledger_directory_never_panics_and_stays_disconnected() {
        let blocker = temp_ledger_dir("blocker");
        std::fs::write(&blocker, "non è una cartella").unwrap();

        let handle = LedgerHandle::begin_in(&blocker, "prova.task", "codex");
        assert!(!handle.is_connected(), "un file al posto della cartella deve far fallire l'apertura");

        handle.record_tokens("codex", "codex", "123");
        handle.finish_went();
        handle.finish_broke("errore", "dettaglio");
        handle.finish_waiting("motore assente");
        handle.finish_skipped("motivo qualsiasi");

        let _ = std::fs::remove_file(&blocker);
    }

    /// I token si scrivono solo quando sono un numero: `"?"` non deve
    /// lasciare una riga con uno zero al posto dell'ignoto.
    #[test]
    fn known_tokens_are_recorded_and_unknown_tokens_write_nothing() {
        let dir = temp_ledger_dir("tokens");
        let handle = LedgerHandle::begin_in(&dir, "prova.task", "codex");
        assert!(handle.is_connected());

        handle.record_tokens("codex", "codex", "?");
        let ledger = Ledger::open(&dir).unwrap();
        let dump = ledger.projection_dump().unwrap();
        assert_eq!(dump["model_calls"].as_array().unwrap().len(), 0, "{dump}");

        handle.record_tokens("codex", "codex", "13910");
        let dump = ledger.projection_dump().unwrap();
        let calls = dump["model_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1, "{calls:?}");
        // `output_tokens` è una colonna testo (per non perdere precisione
        // oltre 2^53, come il resto del deposito): il dump la rende com'è.
        assert_eq!(calls[0][8].as_str(), Some("13910"), "colonna output_tokens: {calls:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La riga di corsa e quella di passo devono raccontare lo stesso esito:
    /// verde nel deposito significa `Outcome::Went` sul passo e `"green"`
    /// sullo stato della corsa.
    #[test]
    fn a_green_finish_writes_matching_run_and_step_outcomes() {
        let dir = temp_ledger_dir("green");
        let handle = LedgerHandle::begin_in(&dir, "prova.task", "openrouter");
        handle.finish_went();

        let ledger = Ledger::open(&dir).unwrap();
        let dump = ledger.projection_dump().unwrap();
        let runs = dump["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0][5].as_str(), Some("green"), "colonna status: {runs:?}");

        let steps = ledger.steps(&handle.run_id).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].outcome, Some(Outcome::Went));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
