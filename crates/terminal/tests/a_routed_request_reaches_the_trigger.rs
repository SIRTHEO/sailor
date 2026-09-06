//! The joint: a request routed by a terminal becomes the signal that starts a
//! flow.
//!
//! **WHY THIS PROOF LIVES APART FROM THE OTHERS.** The `terminal` crate depends
//! on neither `flow` nor `trigger`: a terminal must open even when flows are
//! broken, and letting the flow engine in here would mean opening a shell pulls
//! up the ledger. But a piece that only fits "in theory" does not fit: this
//! proof takes the real output of the routing, hands it to the real manual
//! trigger, and watches what comes out. The two halves touch here, and only
//! here — a test dependency, not a product one.
//!
//! **IT STARTS NO ENGINE.** The trigger step calls nothing and costs nothing:
//! it shapes the signal. The steps that spend are downstream, and proving them
//! would mean paying for real calls at every `cargo test`.

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

/// The manual trigger, called as a flow step would call it.
fn fire(input: Value) -> Value {
    match TriggerAction
        .execute(&input, &SharedState::new())
        .expect("l'innesco manuale è spedito col prodotto")
    {
        ActionOutcome::Went(output) => output,
        ActionOutcome::Waiting(reason) => panic!("nessun innesco resta in attesa: {reason}"),
        ActionOutcome::NotYet(reason) => panic!("no trigger postpones itself: {reason}"),
    }
}

/// **THE WHOLE CHAIN, IN ONE PROOF.** The line typed in a terminal is routed;
/// what comes out becomes the manual trigger's signal; and the fields the
/// downstream steps read hold the request, who wrote it, and in which
/// workspace.
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

    // This is the joint: what the routing produced, put into the trigger's
    // hands. `who` and `where` are what the terminal knows of itself — the
    // reason a terminal is born tied to a workspace.
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

/// **THE FLOW IT SENDS TO REALLY HAS AN ENTRY NODE THAT ACCEPTS A MANUAL
/// SIGNAL.** Without this check a rule could point at a flow beginning with any
/// step at all, and the joint would break on the first real run instead of
/// here.
#[test]
fn the_flow_the_shipped_rules_name_starts_with_a_manual_trigger() {

    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    assert!(!catalog.live().is_empty());
    for loaded in catalog.live() {
        // The flow is looked for where the rule that names it travels: inside
        // the binary, not in this repository's `flows/`.
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
        let document: Value = serde_json::from_str(text).expect("il flusso è JSON");
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

/// A line the routing lets through produces **no** signal: there is nothing to
/// hand any trigger. It is the boundary that makes the joint safe — a fake
/// signal would start the engines downstream, and that costs real calls.
#[test]
fn a_command_produces_no_signal_at_all() {
    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    let router = Router::new(&catalog, Arc::new(NothingIsRunnable));
    match router.route("cd /work/sailor") {
        Routed::Command { .. } => {}
        other => panic!("un comando non deve produrre un segnale: {other:?}"),
    }
}
