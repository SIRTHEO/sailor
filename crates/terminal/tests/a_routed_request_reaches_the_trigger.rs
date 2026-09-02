//! Il collegamento: una richiesta smistata da un terminale diventa il segnale
//! che fa partire un flusso.
//!
//! **PERCHÉ QUESTA PROVA ESISTE SEPARATA DALLE ALTRE.** Il crate `terminal` non
//! dipende né da `flow` né da `trigger`: un terminale deve poter aprirsi anche
//! quando i flussi sono rotti, e far entrare il motore dei flussi qui vorrebbe
//! dire che aprire una shell tira su il deposito. Ma un pezzo che si incastra
//! solo «in teoria» non si incastra: questa prova prende l'uscita vera dello
//! smistamento, la dà all'innesco manuale vero, e guarda cosa ne esce. Le due
//! metà si toccano qui, e solo qui — è una dipendenza di prova, non di prodotto.
//!
//! **NON FA PARTIRE NESSUN MOTORE.** Il passo di innesco non chiama niente e non
//! costa niente: mette in forma il segnale. I passi che spendono stanno a valle,
//! e provarli vorrebbe dire pagare chiamate vere a ogni `cargo test`.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::sync::Arc;
use terminal::{Catalog, CommandLookup, Routed, Router};
use trigger::TriggerAction;

struct NothingIsRunnable;

impl CommandLookup for NothingIsRunnable {
    fn is_command(&self, _word: &str) -> bool {
        false
    }
}

/// L'innesco manuale, invocato come lo invocherebbe un passo di flusso.
fn fire(input: Value) -> Value {
    match TriggerAction
        .execute(&input, &mut SharedState::new())
        .expect("l'innesco manuale è spedito col prodotto")
    {
        ActionOutcome::Went(output) => output,
        ActionOutcome::Waiting(reason) => panic!("nessun innesco resta in attesa: {reason}"),
        ActionOutcome::NotYet(reason) => panic!("no trigger postpones itself: {reason}"),
    }
}

/// **LA CATENA INTERA, IN UNA PROVA SOLA.** La riga scritta in un terminale
/// viene smistata; ciò che ne esce diventa il segnale dell'innesco manuale; e i
/// campi che i passi a valle leggono contengono la richiesta, chi l'ha scritta e
/// in quale spazio di lavoro.
#[test]
fn a_routed_request_becomes_the_signal_of_a_manual_trigger() {
    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    let router = Router::new(&catalog, Arc::new(NothingIsRunnable));

    let Routed::Flow { flow, text, route } =
        router.route("? trova i residui di configurazione rimasti sparsi")
    else {
        panic!("la riga marcata doveva essere smistata");
    };
    assert_eq!(flow, "dispatch-the-work");
    assert_eq!(route, "marked-request");

    // Questo è il giunto: ciò che lo smistamento ha prodotto, messo in mano
    // all'innesco. `who` e `where` sono ciò che il terminale sa di sé — è la
    // ragione per cui un terminale nasce legato a uno spazio di lavoro.
    let signal = fire(json!({
        "source": "manual",
        "text": text,
        "who": "theo",
        "where": "sailor/il-terminale-1"
    }));

    assert_eq!(
        signal["text"],
        "trova i residui di configurazione rimasti sparsi"
    );
    assert_eq!(signal["who"], "theo");
    assert_eq!(signal["where"], "sailor/il-terminale-1");
    assert_eq!(signal["source"], "manual");
    assert_eq!(signal["kind"], "manual");
}

/// **IL FLUSSO A CUI SI MANDA HA DAVVERO UN NODO DI INGRESSO CHE ACCETTA UN
/// SEGNALE MANUALE.** Senza questo controllo, una regola potrebbe puntare a un
/// flusso che comincia con un passo qualunque, e il collegamento si romperebbe
/// alla prima corsa vera invece che qui.
#[test]
fn the_flow_the_shipped_rules_name_starts_with_a_manual_trigger() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("il crate sta due livelli sotto la radice")
        .to_path_buf();

    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    assert!(!catalog.live().is_empty());
    for loaded in catalog.live() {
        // **IL FLUSSO SI CERCA DOVE VIAGGIA LA REGOLA CHE LO NOMINA**, cioè
        // dentro il binario. Fino all'01/09/2026 lo si leggeva da `flows/` del
        // repo: qui c'era, perché era un flusso nostro, e la prova era verde
        // mentre su qualunque altra macchina la regola puntava al nulla.
        let text = flow::system::FLOWS
            .iter()
            .find(|(name, _)| *name == loaded.route.flow)
            .map(|(_, body)| *body)
            .unwrap_or_else(|| {
                panic!(
                    "la regola «{}» manda al flusso «{}», che il prodotto non spedisce: \
                     su una macchina che non è la nostra quella regola non porta da nessuna parte",
                    loaded.route.id, loaded.route.flow
                )
            });
        let document: Value = serde_json::from_str(&text).expect("il flusso è JSON");
        let steps = document["graph"]["steps"]
            .as_array()
            .expect("un grafo ha dei passi");
        let entry = steps
            .iter()
            .find(|step| step["action"] == "trigger")
            .unwrap_or_else(|| {
                panic!(
                    "il flusso «{}» non ha nessun nodo di ingresso: una richiesta smistata non avrebbe da dove entrare",
                    loaded.route.flow
                )
            });
        assert_eq!(
            entry["with"]["source"], "manual",
            "il nodo di ingresso di «{}» non accetta un segnale manuale",
            loaded.route.flow
        );
    }
}

/// Una riga che lo smistamento lascia passare **non** produce nessun segnale:
/// non c'è niente da consegnare a nessun innesco. È il confine che rende sicuro
/// il collegamento — un segnale finto farebbe partire i motori a valle, e costa
/// chiamate vere.
#[test]
fn a_command_produces_no_signal_at_all() {
    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    let router = Router::new(&catalog, Arc::new(NothingIsRunnable));
    match router.route("cd /work/sailor") {
        Routed::Command { .. } => {}
        other => panic!("un comando non deve produrre un segnale: {other:?}"),
    }
}
