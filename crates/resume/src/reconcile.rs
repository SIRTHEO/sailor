//! La parte impura: apre il deposito di Sailor, trova i passi rimasti aperti
//! e scrive il verdetto. Un deposito che non si apre è un avviso nel
//! rapporto, non un errore fatale — la macchina può essere appena ripartita
//! senza che `~/.claude/state/flussi` esista ancora, e questo non deve far
//! cadere chi chiama la ripresa.

use crate::{verdict_for, OpenStep, StepSpecies, Thresholds, Verdict};
use flow::{Completion, Outcome, StepRecord};
use ledger::Ledger;
use std::path::PathBuf;

pub const LEDGER_DIR_ENV: &str = "SAILOR_LEDGER_DIR";

/// `~/.claude/state/flussi`, il deposito predefinito quando la variabile
/// d'ambiente non è impostata.
pub fn default_ledger_dir() -> PathBuf {
    home_dir().join(".claude").join("state").join("flussi")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Il percorso del deposito: `SAILOR_LEDGER_DIR` se impostata, altrimenti il
/// predefinito.
pub fn ledger_dir_from_env() -> PathBuf {
    std::env::var_os(LEDGER_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(default_ledger_dir)
}

/// Cosa si è deciso e scritto per un passo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub verdict: Verdict,
}

/// L'esito di un giro di riconciliazione: cosa è stato applicato, e cosa non
/// si è potuto leggere o scrivere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub applied: Vec<Applied>,
    pub warnings: Vec<String>,
}

/// Come si assegna una specie (e, se noto, il pid detentore) a un passo
/// aperto. Né `StepRecord` né lo schema del deposito portano oggi queste due
/// informazioni — il pid si conosce solo dopo l'avvio, quando l'intenzione è
/// già scritta, e la specie servirebbe una colonna assente da `flow::Step`.
/// `default_classify` risponde perciò sempre "a una persona" e pid ignoto:
/// l'assenza di dati si dichiara, non si inventa. Chi ha il grafo del flusso
/// può passare un classificatore migliore.
pub type Classify = dyn Fn(&StepRecord) -> (StepSpecies, Option<u32>);

pub fn default_classify(_record: &StepRecord) -> (StepSpecies, Option<u32>) {
    (StepSpecies::HandToHuman, None)
}

/// Apre il deposito all'indirizzo di ambiente e riconcilia. Non fa mai
/// cadere il chiamante: un deposito non apribile finisce come avviso.
pub fn reconcile_suspended_steps(
    thresholds: &Thresholds,
    pid_is_alive: &dyn Fn(u32) -> bool,
    classify: &Classify,
    now: i64,
) -> ReconcileReport {
    let dir = ledger_dir_from_env();
    match Ledger::open(&dir) {
        Ok(ledger) => reconcile_ledger(&ledger, thresholds, pid_is_alive, classify, now),
        Err(error) => ReconcileReport {
            applied: Vec::new(),
            warnings: vec![format!("deposito non apribile in {}: {error}", dir.display())],
        },
    }
}

/// Come `reconcile_suspended_steps`, ma su un deposito già aperto — usato
/// dalla prova, che vuole controllare la cartella prima di chiamare.
pub fn reconcile_ledger(
    ledger: &Ledger,
    thresholds: &Thresholds,
    pid_is_alive: &dyn Fn(u32) -> bool,
    classify: &Classify,
    now: i64,
) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    let run_ids = match run_ids(ledger) {
        Ok(ids) => ids,
        Err(error) => {
            report
                .warnings
                .push(format!("elenco delle corse non leggibile: {error}"));
            return report;
        }
    };
    for run_id in run_ids {
        let records = match ledger.steps(&run_id) {
            Ok(records) => records,
            Err(error) => {
                report
                    .warnings
                    .push(format!("passi non leggibili per la corsa {run_id}: {error}"));
                continue;
            }
        };
        for record in records.into_iter().filter(is_open) {
            apply_one(ledger, record, thresholds, pid_is_alive, classify, now, &mut report);
        }
    }
    report
}

#[allow(clippy::too_many_arguments)]
fn apply_one(
    ledger: &Ledger,
    record: StepRecord,
    thresholds: &Thresholds,
    pid_is_alive: &dyn Fn(u32) -> bool,
    classify: &Classify,
    now: i64,
    report: &mut ReconcileReport,
) {
    let (species, held_by_pid) = classify(&record);
    let open_step = OpenStep {
        run_id: record.run_id.clone(),
        step_id: record.step_id.clone(),
        attempt: record.attempt,
        epoch: record.epoch,
        held_by_pid,
        started_at: record.started_at,
        species,
    };
    let verdict = verdict_for(&open_step, now, thresholds, pid_is_alive);
    if let Some(completion) = completion_for(verdict, now) {
        if let Err(error) = ledger.close_step(
            &record.run_id,
            &record.step_id,
            record.attempt,
            record.epoch,
            completion,
        ) {
            report.warnings.push(format!(
                "chiusura fallita per {}/{} tentativo {}: {error}",
                record.run_id, record.step_id, record.attempt
            ));
            return;
        }
    }
    report.applied.push(Applied {
        run_id: record.run_id,
        step_id: record.step_id,
        attempt: record.attempt,
        verdict,
    });
}

fn is_open(record: &StepRecord) -> bool {
    record.outcome.is_none() && record.ended_at.is_none()
}

/// Cosa scrivere nel deposito per ciascun verdetto. `StillRunning` non scrive
/// nulla: è l'unico caso in cui "non toccare" è la decisione stessa. Gli
/// altri riusano gli esiti che il motore già conosce — `Broke` è ciò che
/// rende un passo di nuovo pronto quando i tentativi non sono esauriti,
/// `Stopped` è il finale già previsto per "fermato, non rotto", `Waiting` è
/// già "non si può concludere da soli".
fn completion_for(verdict: Verdict, now: i64) -> Option<Completion> {
    let (outcome, said, failure_class) = match verdict {
        Verdict::StillRunning => return None,
        Verdict::Resume => (
            Outcome::Broke,
            "ripresa dopo riavvio: pid morto, passo ripetibile",
            "resumed_after_restart",
        ),
        Verdict::Redo => (
            Outcome::Broke,
            "ripresa dopo riavvio: pid morto, passo compensabile",
            "compensate_and_retry",
        ),
        Verdict::AbandonMarked => (
            Outcome::Stopped,
            "abbandonato dalla riconciliazione all'avvio",
            "abandoned_by_resume",
        ),
        Verdict::HandToHuman => (
            Outcome::Waiting,
            "non ripetibile e non compensabile: serve una persona",
            "needs_human",
        ),
    };
    Some(Completion {
        outcome,
        output: None,
        said: Some(said.to_owned()),
        failure_class: Some(failure_class.to_owned()),
        ended_at: now,
        bytes_seen: None,
        bytes_discarded: None,
    })
}

/// Gli identificativi di corsa distinti fra i passi già scritti. Si guarda
/// `steps`, non `runs`: un passo può esistere prima che la corsa che lo
/// contiene sia stata registrata a parte, e la ripresa non deve dipendere da
/// quell'ordine.
fn run_ids(ledger: &Ledger) -> Result<Vec<String>, ledger::LedgerError> {
    let dump = ledger.projection_dump()?;
    let steps = dump
        .get("steps")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<String> = steps
        .into_iter()
        .filter_map(|row| row.get(0).and_then(|value| value.as_str()).map(str::to_owned))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::StepRecord;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "resume-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("creare la cartella di prova");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dead(_pid: u32) -> bool {
        false
    }

    fn repeatable_held_by_dead_pid(_record: &StepRecord) -> (StepSpecies, Option<u32>) {
        (StepSpecies::Repeatable, Some(999_999))
    }

    #[test]
    fn an_open_step_from_a_dead_pid_gets_closed_and_becomes_retriable() {
        let directory = TestDirectory::new("dead-pid");
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito di prova");
        let started = StepRecord::started(
            "run-suspended",
            "compile",
            1,
            1,
            vec![],
            serde_json::json!(null),
            vec![],
            10,
        );
        ledger
            .append_step_started(&started)
            .expect("scrivere l'intenzione");

        let before = ledger
            .steps("run-suspended")
            .expect("leggere i passi prima");
        assert_eq!(before[0].outcome, None, "il passo parte aperto");

        let report = reconcile_ledger(
            &ledger,
            &Thresholds::default(),
            &dead,
            &repeatable_held_by_dead_pid,
            100,
        );

        assert!(report.warnings.is_empty(), "nessun avviso atteso: {report:?}");
        assert_eq!(report.applied.len(), 1);
        assert_eq!(report.applied[0].verdict, Verdict::Resume);

        let after = ledger.steps("run-suspended").expect("leggere i passi dopo");
        assert_eq!(
            after[0].outcome,
            Some(Outcome::Broke),
            "il passo deve risultare chiuso, pronto per un nuovo tentativo"
        );
        assert!(after[0].ended_at.is_some());
    }

    #[test]
    fn a_step_held_by_a_live_pid_is_left_open() {
        let directory = TestDirectory::new("live-pid");
        let ledger = Ledger::open(&directory.0).expect("aprire il deposito di prova");
        let started = StepRecord::started(
            "run-live",
            "compile",
            1,
            1,
            vec![],
            serde_json::json!(null),
            vec![],
            10,
        );
        ledger
            .append_step_started(&started)
            .expect("scrivere l'intenzione");

        // Il pid del processo di prova stesso: certamente vivo mentre gira.
        let own_pid = std::process::id();
        let classify = move |_record: &StepRecord| (StepSpecies::Repeatable, Some(own_pid));
        let report = reconcile_ledger(
            &ledger,
            &Thresholds::default(),
            &notte::process_exists,
            &classify,
            100,
        );

        assert_eq!(report.applied[0].verdict, Verdict::StillRunning);
        let after = ledger.steps("run-live").expect("leggere i passi dopo");
        assert_eq!(after[0].outcome, None, "un passo al lavoro non si tocca");
    }

    #[test]
    fn unreadable_ledger_directory_is_a_warning_not_a_panic() {
        // `HOME` inesistente e nessuna variabile d'ambiente: il percorso
        // predefinito punta sotto una home vuota, che comunque `Ledger::open`
        // crea da sé — la prova che conta è che non si va mai in panico, anche
        // quando l'apertura fallisce per un altro motivo (qui: `SAILOR_LEDGER_DIR`
        // puntato su un file, non una cartella).
        let directory = TestDirectory::new("not-a-directory");
        let file_path = directory.0.join("this-is-a-file");
        std::fs::write(&file_path, b"ostacolo").expect("creare l'ostacolo");
        // SAFETY: la prova è a thread singolo per questa variabile; nessun'altra
        // prova in questo processo la legge o la scrive in parallelo.
        unsafe {
            std::env::set_var(LEDGER_DIR_ENV, &file_path);
        }
        let report =
            reconcile_suspended_steps(&Thresholds::default(), &dead, &default_classify, 100);
        unsafe {
            std::env::remove_var(LEDGER_DIR_ENV);
        }
        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1, "un avviso, non un fallimento: {report:?}");
    }

    #[test]
    fn default_ledger_dir_is_under_home_state_flussi() {
        let path = default_ledger_dir();
        assert!(path.ends_with(".claude/state/flussi"));
    }

    #[test]
    fn default_classify_hands_every_step_to_a_human_with_no_known_pid() {
        let started = StepRecord::started(
            "run", "step", 1, 1, vec![], serde_json::json!(null), vec![], 1,
        );
        assert_eq!(
            default_classify(&started),
            (StepSpecies::HandToHuman, None)
        );
    }
}
