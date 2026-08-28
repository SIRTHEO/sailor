//! I due nodi con cui un flusso ricorda qualcosa fra una corsa e l'altra.
//!
//! **PERCHÉ ESISTONO.** Il motore sa eseguire un grafo, ma fino al 28/08/2026
//! non sapeva *ricordare* niente che non fosse una corsa, un passo o una
//! chiamata a un modello. Ogni fatto che una lavorazione doveva tenere — su che
//! lavoro si è, quando è girata l'ultima volta, cosa aveva già visto — finiva
//! in una struttura Rust scritta apposta. È il modo in cui `notte` è diventata
//! 2.562 righe per un flusso di quattro passi, ed è il motivo per cui è
//! condannata.
//!
//! Qui il fatto sta in una **collezione** che nomina chi scrive il flusso, e i
//! due nodi sono gli unici che la toccano. Il motore non sa cosa significhi
//! quel nome, e non deve saperlo: sa tenerlo.
//!
//! **Il deposito arriva alla registrazione, non all'esecuzione.** Un'azione
//! riceve solo il proprio ingresso e lo stato condiviso — è il contratto che
//! tiene `flow` agnostico rispetto a qualunque servizio — quindi chi registra
//! questi due nodi gli consegna il deposito su cui lavorare. Un flusso non può
//! scegliere un deposito diverso da quello di chi lo esegue: lo spazio dei nomi
//! è suo, il file no.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// Il nome sotto cui `StoreWriteAction` si registra.
pub const STORE_WRITE_ACTION: &str = "store_write";
/// Il nome sotto cui `StoreReadAction` si registra.
pub const STORE_READ_ACTION: &str = "store_read";

/// Registra entrambi i nodi del deposito sul deposito dato.
pub fn register_store(registry: &mut flow::ActionRegistry, ledger: Ledger) {
    registry.register(STORE_WRITE_ACTION, StoreWriteAction::new(ledger.clone()));
    registry.register(STORE_READ_ACTION, StoreReadAction::new(ledger));
}

#[derive(Debug, Deserialize)]
struct WriteSpec {
    collection: String,
    key: String,
    value: Value,
    /// Chi lo sta scrivendo. Il flusso lo dichiara perché chi rilegge la voce
    /// sappia da dove viene: una voce senza autore si può leggere, ma non si
    /// può contestare.
    written_by: String,
    /// L'istante, se il flusso lo detta. Serve alle prove, che altrimenti
    /// dipenderebbero dall'orologio, e a chi ridichiara un fatto già datato.
    #[serde(default)]
    written_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReadSpec {
    collection: String,
    key: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Scrive una voce nella collezione che il flusso ha nominato.
///
/// **Ripetibile per costruzione**: la voce è identificata da collezione e
/// chiave, quindi riscrivere lo stesso valore lascia il deposito com'era. È la
/// ragione per cui questo nodo può essere rilanciato senza consegnare niente a
/// una persona — a differenza di un nodo che manda una riga a un terminale, che
/// il mondo non sa disfare.
pub struct StoreWriteAction {
    ledger: Ledger,
}

impl StoreWriteAction {
    pub fn new(ledger: Ledger) -> Self {
        Self { ledger }
    }
}

impl Action for StoreWriteAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let spec: WriteSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let record = StoreRecord {
            collection: spec.collection,
            key: spec.key,
            value: spec.value,
            written_by: spec.written_by,
            written_at: spec.written_at.unwrap_or_else(now),
        };
        // Un indirizzo vuoto è un errore di chi ha scritto il flusso, non un
        // dato del mondo: si dice subito con le parole del deposito.
        self.ledger
            .put_record(&record)
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "collection": record.collection,
            "key": record.key,
            "written_at": record.written_at,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Legge una voce, e dice **se c'era**.
///
/// Una voce che nessuno ha ancora scritto non è un fallimento del passo: è la
/// risposta. Chi la riceve ha un ramo per il caso «non lo so» — che è
/// esattamente il caso in cui una lavorazione gira per la prima volta, e il
/// caso in cui, prima di questo nodo, qualcuno si sarebbe messo a indovinare.
pub struct StoreReadAction {
    ledger: Ledger,
}

impl StoreReadAction {
    pub fn new(ledger: Ledger) -> Self {
        Self { ledger }
    }
}

impl Action for StoreReadAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let spec: ReadSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let found = self
            .ledger
            .read_record(&spec.collection, &spec.key)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        Ok(ActionOutcome::Went(match found {
            Some(record) => json!({
                "found": true,
                "value": record.value,
                "written_by": record.written_by,
                "written_at": record.written_at,
            }),
            None => json!({ "found": false }),
        }))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestStore(std::path::PathBuf);

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store() -> (Ledger, TestStore) {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("sailor-actions-store-{}-{sequence}", std::process::id()));
        let ledger = Ledger::open(&path).expect("aprire il deposito");
        (ledger, TestStore(path))
    }

    /// Il giro intero, dal nodo che scrive al nodo che legge.
    ///
    /// Non è una prova di due funzioni: è la prova che un flusso può ricordare
    /// **senza che il motore sappia cosa** — la collezione qui si chiama
    /// `mandate`, e quella parola non compare in nessun punto del codice che
    /// esegue questi due nodi.
    #[test]
    fn a_flow_can_remember_something_the_engine_knows_nothing_about() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let write = StoreWriteAction::new(ledger.clone());
        let read = StoreReadAction::new(ledger);

        write
            .execute(
                &json!({
                    "collection": "mandate",
                    "key": "current",
                    "value": {"file": "2026-08-28-sailor.md"},
                    "written_by": "flusso-mandato-corrente",
                    "written_at": 1_756_400_000i64,
                }),
                &mut shared,
            )
            .expect("scrittura");

        let outcome = read
            .execute(&json!({"collection": "mandate", "key": "current"}), &mut shared)
            .expect("lettura");
        let ActionOutcome::Went(value) = outcome else {
            panic!("un nodo che legge un deposito locale non aspetta nessuno");
        };
        assert_eq!(value["found"], json!(true));
        assert_eq!(value["value"], json!({"file": "2026-08-28-sailor.md"}));
        assert_eq!(value["written_by"], json!("flusso-mandato-corrente"));
    }

    /// Una voce mai scritta risponde `found: false`, e il passo **riesce**.
    ///
    /// Il mutante che la fa cadere è trasformare l'assenza in un
    /// `ActionError`: il flusso non avrebbe più un ramo per il primo giro, e
    /// una lavorazione nuova nascerebbe rossa.
    #[test]
    fn a_missing_record_is_an_answer_not_a_failure() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let read = StoreReadAction::new(ledger);

        let outcome = read
            .execute(&json!({"collection": "mandate", "key": "current"}), &mut shared)
            .expect("la lettura di una voce assente non è un errore");
        let ActionOutcome::Went(value) = outcome else {
            panic!("nessuna attesa");
        };
        assert_eq!(value["found"], json!(false));
        assert_eq!(value.get("value"), None, "non si inventa un valore che nessuno ha scritto");
    }

    /// Un indirizzo vuoto è un errore di chi ha scritto il flusso.
    ///
    /// Il deposito lo rifiuta, e il nodo riporta il rifiuto invece di
    /// depositare una voce che nessuno ritroverà.
    #[test]
    fn an_empty_address_is_refused_with_the_stores_own_words() {
        let (ledger, _guard) = store();
        let mut shared = SharedState::new();
        let write = StoreWriteAction::new(ledger);

        let error = write
            .execute(
                &json!({
                    "collection": "",
                    "key": "current",
                    "value": 1,
                    "written_by": "prova",
                }),
                &mut shared,
            )
            .expect_err("una collezione vuota non si scrive");
        assert_eq!(error.class, "store_refused");
    }
}
