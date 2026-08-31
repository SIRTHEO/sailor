//! Ciò che serve al passo `subflow` e che il crate del flusso non può avere.
//!
//! Tre cose: **dove** si cercano i flussi, **con che azioni** far girare quello
//! interno, e **dove** si scrive la corsa figlia. La prima e la terza vogliono
//! il deposito, che `flow` non conosce di proposito — `ledger` dipende da
//! `flow`, non il contrario. La seconda è un anello: il passo deve girare con
//! il registro in cui esso stesso è registrato, e un riferimento diretto non si
//! può costruire.
//!
//! **COME SI CHIUDE L'ANELLO, E QUANTO COSTA.** Il registro del figlio si
//! costruisce **quando serve**, non quando si costruisce quello del padre, e si
//! tiene: un livello di annidamento, un registro. Non è gratis — ogni registro
//! rileva di nuovo gli strumenti della macchina — ma succede solo se qualcuno
//! annida davvero, ed è limitato da `flow::subflow::MAX_DEPTH`. L'alternativa
//! era rendere `Arc<ActionRegistry>` il tipo che tutti si passano, cioè toccare
//! la riga di comando e il guscio della finestra per un dettaglio interno.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use flow::subflow::{RunNote, SubflowHost};
use flow::system::{self, FlowSource};
use flow::{ActionError, ActionRegistry, Execution, RecordStore};
use ledger::Ledger;

use crate::{default_registry, record_child_run, stopped_by_cap, FlowRun};

/// Il «dove» della sorgente *tuoi* quando questa macchina non dichiara una casa.
///
/// **NON È UN PERCORSO VUOTO, ED È IL PUNTO.** Un percorso vuoto diventa il
/// relativo `flows`, cioè la cartella corrente: i flussi del progetto
/// comparirebbero anche come «tuoi», e a parità di nome vincerebbe quello
/// sbagliato. Un percorso assoluto che non esiste legge zero flussi, che è la
/// risposta vera.
const NO_HOME: &str = "/sailor-non-ha-una-casa-su-questa-macchina";

/// Il ponte fra il passo `subflow` e il resto di Sailor.
pub struct LedgerHost {
    /// `None` quando chi ha costruito il registro non ha un deposito: allora il
    /// passo non esegue, e dice perché.
    ledger: Option<Ledger>,
    watcher: Option<Arc<dyn actions::StepSinks>>,
    /// Il registro con cui gira il figlio, costruito alla prima chiamata.
    nested: OnceLock<Arc<ActionRegistry>>,
}

impl LedgerHost {
    pub fn new(ledger: Option<Ledger>, watcher: Option<Arc<dyn actions::StepSinks>>) -> Self {
        Self {
            ledger,
            watcher,
            nested: OnceLock::new(),
        }
    }

    /// Il deposito, o l'errore che dice perché senza non si esegue.
    ///
    /// **NON SI ESEGUE SENZA DEPOSITO, E NON È UNA LIMITAZIONE TECNICA.** Un
    /// figlio girerebbe benissimo su un deposito in memoria; quello che non
    /// potrebbe fare è **essere risalito** dal passo che l'ha chiamato, che è
    /// la decisione 4. Un lavoro che sparisce dentro un altro è l'opacità che
    /// questo prodotto esiste per togliere: meglio un errore leggibile che una
    /// corsa figlia di cui nessuno saprà mai niente.
    fn deposit(&self) -> Result<&Ledger, ActionError> {
        self.ledger.as_ref().ok_or_else(|| {
            ActionError::new(
                "no_ledger",
                "senza deposito la corsa figlia non sarebbe risalibile dal passo che l'ha chiamata",
            )
        })
    }
}

impl SubflowHost for LedgerHost {
    /// **LE STESSE SORGENTI DI `sailor flow run`, NON UNA SECONDA REGOLA.** La
    /// precedenza — *di sistema* < *tuoi* < *del progetto* — vive in
    /// `flow::system::sources_from_env`. Se un `subflow` cercasse altrove, due
    /// macchine eseguirebbero flussi diversi con lo stesso nome senza dirlo.
    fn sources(&self) -> Vec<FlowSource> {
        let home = ledger::sailor_home()
            .map(|home| home.join("flows"))
            .unwrap_or_else(|| PathBuf::from(NO_HOME));
        system::sources_from_env(&home)
    }

    /// Le azioni del figlio sono quelle del padre, costruite alla prima
    /// chiamata e poi tenute.
    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        Ok(self
            .nested
            .get_or_init(|| {
                Arc::new(default_registry(
                    self.ledger.clone(),
                    self.watcher.clone(),
                ))
            })
            .clone())
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::new(self.deposit()?.clone()) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError> {
        // CHI L'HA AVVIATA È UN PASSO, E SI LEGGE. Il deposito distingue così
        // una corsa figlia da una lanciata a mano che porta lo stesso flusso.
        let started_by = format!("subflow {}", note.parent_step_id);
        record_child_run(
            self.deposit()?,
            note.flow,
            FlowRun {
                run_id: note.run_id,
                status: note.status,
                started_at: note.started_at,
                ended_at: note.ended_at,
                error: note.error.clone(),
                started_by: &started_by,
            },
            note.parent_run_id,
        )
        .map_err(|said| ActionError::new("child_run_not_recorded", said))
    }

    fn why(&self, execution: &Execution) -> Option<String> {
        stopped_by_cap(execution)
    }
}
