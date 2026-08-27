//! Lo specchio: ogni lavorazione scritta anche nel deposito durevole, mentre il
//! registro di testo continua a essere l'unica cosa che qualcuno legge.
//!
//! PERCHÉ ESISTE, e perché è un gradino e non la conversione. Deciso da Theo il
//! 27/08/2026: *«stiamo costruendo un sistema centralizzato che permette
//! all'utente anche di vedere e capire i flussi e tutto deve essere convertito
//! così»*. La misura presa quel giorno dice da dove si parte: il motore dei
//! flussi è costruito e provato, e **sul disco non esiste nessun deposito** —
//! nessun flusso ha mai girato. Il servizio che lavora tutto il giorno è
//! l'unica cosa che produce lavorazioni vere, dodici, e non ne fa parte.
//!
//! Il primo gradino non sposta niente: **scrive in più**. Il deposito si riempie
//! di una giornata di storia vera mentre il servizio continua a funzionare
//! esattamente come prima, e a sera le due storie si confrontano. Se non dicono
//! la stessa cosa, è il momento di scoprirlo — non dopo, quando il registro di
//! testo sarà già stato tolto di mezzo.
//!
//! **NIENTE DI QUI DENTRO PUÒ FAR CADERE UNA LAVORAZIONE.** È la proprietà che
//! rende accettabile il gradino: uno specchio che rompe ciò che riflette è
//! peggio dell'assenza dello specchio. Qui si compongono i record e basta; chi
//! li scrive ignora ogni errore, e il registro di testo resta la fonte finché
//! qualcuno non decide il contrario.
//!
//! L'IDENTITÀ DI UN'ESECUZIONE non può essere il solo nome del compito: una
//! lavorazione ricorrente torna in coda ogni giorno e ne avrebbe una sola per
//! sempre. Porta con sé il giorno e il processo che l'ha presa — lo stesso
//! criterio con cui il servizio nomina già le ricevute in `in-corso/`.

use flow::{digest_input, Completion, Outcome as StepOutcome, StepRecord};
use serde_json::json;

use crate::{strip_receipt_suffix, Outcome};

/// Il nome del passo dentro l'esecuzione. Oggi il ciclo esegue una cosa sola per
/// giro, quindi ce n'è uno; quando il ciclo diventerà un grafo — scegli,
/// esegui, verifica, scrivi l'esito — questo sarà il secondo di quattro.
pub const STEP: &str = "esegui";

/// L'identità di questa esecuzione: compito, giorno, processo.
///
/// Il nome del compito perde il suffisso della ricevuta, perché `claim_task`
/// glielo attacca e due esecuzioni dello stesso compito non devono sembrare
/// compiti diversi.
pub fn run_id(task_name: &str, today: &str, pid: u32) -> String {
    format!("notte/{}/{today}/{pid}", strip_receipt_suffix(task_name))
}

/// L'intenzione, da scrivere **prima** di chiamare il motore.
///
/// `gates` porta i freni attivi: qui è il peso dichiarato dal compito, che è
/// l'unica cosa che ha deciso se questo giro poteva partire con la macchina
/// occupata. Senza, un passo ripreso sembrerebbe identico a uno partito sotto
/// condizioni diverse.
pub fn step_started(
    run_id: &str,
    task_name: &str,
    engine: &str,
    weight: &str,
    started_at: i64,
) -> StepRecord {
    let input = json!({
        "compito": strip_receipt_suffix(task_name),
        "motore": engine,
    });
    StepRecord {
        run_id: run_id.to_string(),
        step_id: STEP.to_string(),
        attempt: 1,
        epoch: 1,
        deps: Vec::new(),
        input_digest: digest_input(&input),
        input,
        gates: vec![format!("peso:{weight}")],
        attempt_relation: None,
        started_at,
        outcome: None,
        output: None,
        said: None,
        failure_class: None,
        ended_at: None,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

/// L'esito, da scrivere **dopo**.
///
/// LA CLASSE DEL GUASTO NON È UNA DIAGNOSI, ed è la ragione per cui il rosso si
/// divide in due: un compito che cade perché il motore non risponde — quota
/// esaurita, riga di comando assente — non ha niente a che vedere con uno la cui
/// verifica dice di no. Il 27/08/2026 cinque rossi su cinque erano la prima
/// specie, e dal registro di testo si distinguono solo leggendo la frase.
pub fn completion(outcome: &Outcome, ended_at: i64) -> Completion {
    let (step_outcome, failure_class, said) = match outcome {
        Outcome::Green { engine_label, tokens, seconds } => (
            StepOutcome::Went,
            None,
            Some(format!("{engine_label} · {tokens} token · {seconds}s")),
        ),
        Outcome::Red { engine_label, tokens, seconds, reason } => (
            StepOutcome::Broke,
            Some(failure_class(reason).to_string()),
            Some(format!("{engine_label} · {tokens} token · {seconds}s · {reason}")),
        ),
        Outcome::Skipped { reason } => (StepOutcome::Skipped, None, Some(reason.clone())),
        Outcome::Deferred { reason } => (StepOutcome::Waiting, None, Some(reason.clone())),
    };
    Completion {
        outcome: step_outcome,
        output: None,
        said,
        failure_class,
        ended_at,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

/// A quale specie appartiene un rosso. Tre classi, non una diagnosi: chi conta i
/// guasti deve poter separare «il motore non c'era» da «il lavoro non ha
/// funzionato» senza leggere una frase in italiano.
fn failure_class(reason: &str) -> &'static str {
    let reason = reason.to_lowercase();
    if reason.contains("motore") {
        "motore"
    } else if reason.contains("timeout") || reason.contains("scaduto") {
        "tempo"
    } else {
        "verifica"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recurring_task_gets_one_identity_per_day_and_process() {
        let first = run_id("triage.task.4242", "2026-08-27", 4242);
        let second = run_id("triage.task.9999", "2026-08-28", 9999);
        assert_eq!(first, "notte/triage.task/2026-08-27/4242");
        assert_ne!(first, second, "due giorni non possono essere la stessa esecuzione");
    }

    /// L'intenzione si scrive prima, quindi l'esito deve essere assente: un
    /// record che nasce già chiuso non distingue un passo finito da uno
    /// interrotto a metà, che è tutto il punto del deposito.
    #[test]
    fn the_intention_is_written_open() {
        let record = step_started("notte/x/2026-08-27/1", "x.task", "codex", "leggero", 100);
        assert!(record.outcome.is_none());
        assert!(record.ended_at.is_none());
        assert_eq!(record.gates, vec!["peso:leggero".to_string()]);
        assert!(!record.input_digest.is_empty());
    }

    /// Lo stesso compito con lo stesso motore ha la stessa impronta d'ingresso,
    /// anche se il nome porta il suffisso della ricevuta di un altro processo.
    #[test]
    fn the_receipt_suffix_does_not_change_the_input() {
        let one = step_started("r", "x.task.111", "codex", "leggero", 1);
        let other = step_started("r", "x.task.222", "codex", "leggero", 2);
        assert_eq!(one.input_digest, other.input_digest);
    }

    #[test]
    fn green_closes_as_went_and_red_as_broke() {
        let green = Outcome::Green {
            engine_label: "codex".into(),
            tokens: "13543".into(),
            seconds: 78,
        };
        assert_eq!(completion(&green, 200).outcome, StepOutcome::Went);

        let red = Outcome::Red {
            engine_label: "codex".into(),
            tokens: "?".into(),
            seconds: 8,
            reason: "motore: errore".into(),
        };
        let closed = completion(&red, 200);
        assert_eq!(closed.outcome, StepOutcome::Broke);
        assert_eq!(closed.failure_class.as_deref(), Some("motore"));
    }

    /// Il caso che il 27/08 si distingueva solo leggendo l'italiano: cinque
    /// rossi del motore e uno della verifica, nella stessa colonna.
    #[test]
    fn a_failed_check_is_a_different_class_from_a_missing_engine() {
        let check = Outcome::Red {
            engine_label: "codex".into(),
            tokens: "27417".into(),
            seconds: 269,
            reason: "verifica: timeout dopo 120s".into(),
        };
        assert_eq!(completion(&check, 1).failure_class.as_deref(), Some("tempo"));

        let refused = Outcome::Red {
            engine_label: "codex".into(),
            tokens: "100".into(),
            seconds: 5,
            reason: "verifica: uscita 1".into(),
        };
        assert_eq!(completion(&refused, 1).failure_class.as_deref(), Some("verifica"));
    }

    /// Un compito rimandato non è caduto: resta in attesa, e chi conta i guasti
    /// non deve trovarselo dentro.
    #[test]
    fn a_deferred_task_waits_instead_of_breaking() {
        let deferred = Outcome::Deferred { reason: "motore assente".into() };
        assert_eq!(completion(&deferred, 1).outcome, StepOutcome::Waiting);
        assert!(completion(&deferred, 1).failure_class.is_none());
    }
}
