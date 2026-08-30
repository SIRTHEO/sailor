//! Da dove arriva il segnale che fa partire un flusso.
//!
//! **PERCHÉ ESISTE UN NODO DI INGRESSO.** Un grafo di passi non dice mai da
//! dove viene il lavoro: il primo nodo riceveva la consegna come una costante
//! scritta dentro il file, e cambiare lavoro voleva dire riscrivere il flusso.
//! Un innesco è un passo senza dipendenze che **attende un segnale** e mette a
//! disposizione dei passi a valle ciò che il segnale portava: il testo della
//! consegna, chi l'ha mandata, da dove. Da lì in poi il grafo è lo stesso, e la
//! consegna è un dato che entra.
//!
//! **LE SORGENTI SONO UN ELENCO, NON UN `match`.** In questo crate non compare
//! il nome di nessun terminale, di nessun prodotto e di nessun percorso di
//! questa macchina: il codice conosce due *forme* di sorgente — una che porta
//! il segnale con sé (manuale) e una che lo vedrebbe comparire in una sessione
//! di terminale — e quali terminali esistano lo dicono i descrittori, che si
//! aggiungono scrivendo una riga di JSON.
//!
//! **IL CONFINE, DICHIARATO INVECE CHE SIMULATO.** L'innesco manuale è vero:
//! il segnale è ciò che il lanciatore gli ha messo in mano, ed è la sorgente
//! che il pulsante della finestra userà. L'innesco da terminale **non ascolta
//! niente**, e non finge: la sua forma è definita e si carica, ma il passo si
//! ferma con un errore che dice esattamente cosa manca perché diventi reale.
//! Un ascolto simulato sarebbe peggio di un ascolto assente, perché un flusso
//! verde direbbe che qualcuno ha parlato.

pub mod action;
pub mod descriptor;

pub use action::{register_default, TriggerAction, TRIGGER_ACTION};
pub use descriptor::{Catalog, Kind, Listen, Loaded, Problem, Source, TriggerDescriptor};

use serde::Serialize;
use std::path::PathBuf;
use toolbox::Machine;

/// Ciò che un segnale portava, nella forma che i passi a valle leggono.
///
/// **OGNI CAMPO È UN TESTO, ANCHE QUANDO È VUOTO.** Un campo assente o nullo
/// romperebbe il rinvio `$join` del passo dopo — che unisce testo e rifiuta
/// tutto il resto — e la rottura arriverebbe a un passo che non c'entra. Un
/// segnale che non sa chi l'ha mandato lo dice con una stringa vuota, e chi
/// compone il messaggio decide cosa farne.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signal {
    /// La consegna: il testo che il segnale portava.
    pub text: String,
    /// Chi l'ha mandata, per come la sorgente lo sa. Vuoto se non lo sa.
    pub who: String,
    /// Da dove: la sessione, il pannello, la finestra. Vuoto se non lo sa.
    #[serde(rename = "where")]
    pub where_from: String,
    /// L'`id` del descrittore che ha riconosciuto il segnale.
    pub source: String,
    /// La forma della sorgente: `manual`, `terminal`.
    pub kind: String,
}

/// Le sorgenti da cui si prendono i descrittori degli inneschi.
///
/// Stesse regole dei descrittori degli strumenti, e di proposito: chi ha
/// imparato dove si aggiunge una riga di comando non deve impararlo una seconda
/// volta per aggiungere un innesco. Nell'ordine in cui vincono: prima quelli
/// spediti, poi quelli dell'utente.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(
        toolbox::sailor_home_for(machine).join("triggers.d"),
    ));
    if let Some(extra) = machine.env.get("SAILOR_TRIGGER_DESCRIPTORS") {
        for raw in extra.split(':').filter(|s| !s.is_empty()) {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                out.push(Source::Dir(path));
            } else {
                out.push(Source::File(path));
            }
        }
    }
    out
}
