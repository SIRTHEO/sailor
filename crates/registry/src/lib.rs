//! Ciò che la riga di comando e la finestra devono fare allo stesso modo.
//!
//! Due cose, oggi: **quali azioni** Sailor sa eseguire, e **come si registra
//! l'intestazione di una corsa**. Sono entrambe arrivate qui per la stessa
//! strada — erano scritte due volte e si sono disallineate — e chi ne trova una
//! terza la porti qui invece di ricopiarla.
//!
//! **PERCHÉ QUESTO CRATE ESISTE.** Questa lista viveva in due copie: una nel
//! comando `sailor flow run`, una nel guscio della finestra. Le due si sono
//! disallineate almeno tre volte, e l'ultima è del 30/08/2026 alle 09:05, quando
//! il motore ha imparato a registrare quanto spende — da una parte sola. Il
//! risultato non era un errore di compilazione né una prova rossa: era una
//! finestra che, lanciando **lo stesso flusso** del terminale, non sapeva
//! risolvere gli strumenti per identificativo, non scriveva nessun costo nel
//! deposito, e rifiutava come «azione sconosciuta» due nodi che dal terminale
//! funzionavano. Tre comportamenti diversi per lo stesso file di flusso, e
//! nessun modo di accorgersene se non provandoli tutti e due.
//!
//! Chi aggiunge un'azione la aggiunge qui, e la trovano tutti.

mod run_record;

pub use run_record::{
    execution_status, record_flow_run, stopped_by_cap, why_it_stopped, FlowRun,
};

use std::path::Path;
use std::sync::Arc;

use flow::{ActionRegistry, ExecutionRequest, FlowFile, SharedState};
use ledger::Ledger;

/// La richiesta con cui una corsa parte, costruita **una volta sola**.
///
/// **PERCHÉ STA QUI E NON NEI DUE CHIAMANTI.** Era scritta due volte — nel
/// comando `sailor flow run` e nel guscio della finestra — ed è esattamente la
/// forma del guasto 10: le due copie si sono già disallineate almeno tre volte,
/// l'ultima con `total_cost_micros: 0` scritto a mano da una parte sola. La
/// radice del progetto è il dato nuovo che le avrebbe fatte divergere di nuovo,
/// e nel modo peggiore: una corsa lanciata dalla finestra che lavora dove sta il
/// processo mentre la stessa corsa dal terminale lavora nella radice giusta.
///
/// **LA RADICE PUÒ MANCARE, E ALLORA NON C'È.** `None` non diventa la cartella
/// corrente: chi ha bisogno di una radice fallisce dicendolo, al momento di
/// comporre l'ingresso del passo. Un ripiego silenzioso qui rimetterebbe in
/// piedi il guasto 25 nel punto esatto che deve chiuderlo.
pub fn execution_request(flow: &FlowFile, run_id: &str, root: Option<&Path>) -> ExecutionRequest {
    let mut shared = SharedState::new();
    if let Some(root) = root {
        // Il tipo del valore lo dà `SharedState`: così questo crate resta senza
        // dipendenze esterne proprie, che è la ragione per cui esiste.
        shared.insert(
            flow::WORKSPACE_ROOT.to_owned(),
            root.display().to_string().into(),
        );
    }
    ExecutionRequest {
        run_id: run_id.to_owned(),
        root_inputs: flow.inputs.clone(),
        gates: Vec::new(),
        shared,
        // Il tetto è del flusso e viaggia con la corsa: chi lancia non lo
        // inventa, lo porta.
        spend_cap_micros: flow.spend_cap_micros,
    }
}

/// Il registro delle azioni: tutto ciò che un passo può chiedere di fare.
///
/// **L'ORDINE DELLE RIGHE CONTA, E NON È CASUALE.** `actions::register_default`
/// registra un motore esterno che non sa risolvere uno strumento per
/// identificativo; la riga più sotto lo *sostituisce* con uno che lo sa. Chi
/// inverte le due righe ottiene un registro che compila, gira, e fallisce ogni
/// passo che nomina uno strumento invece di un binario.
///
/// **IL DEPOSITO È FACOLTATIVO E LA DIFFERENZA È DICHIARATA.** Chi esegue lo
/// passa e ottiene le righe di consumo; chi fa un controllo statico non ce l'ha
/// e non deve averlo — aprire un deposito per controllare un grafo creerebbe
/// file sul disco per una domanda che non tocca niente. I nodi che *scrivono*
/// nel deposito restano fuori quando manca, e `flow check` lo dice; quello che
/// *legge* lo storico si registra comunque, perché «non c'è nessuna corsa
/// registrata» è una risposta buona e non un guasto.
pub fn default_registry(
    ledger: Option<Ledger>,
    watcher: Option<Arc<dyn actions::StepSinks>>,
) -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    // Il rilevamento di cosa c'è sulla macchina è un'azione come le altre: un
    // passo può chiedere «che strumenti ho qui» invece di dare per scontato che
    // ci siano.
    toolbox::register_default(&mut registry);
    // «Questi flussi girano qui?»: la metà mancante del rilevamento, perché un
    // elenco di cosa c'è non dice a nessuno cosa smetterà di funzionare.
    toolbox::register_needs(&mut registry);
    // Da dove arriva il segnale che fa partire un flusso: anche le sorgenti sono
    // un elenco di descrittori, non un ramo di codice.
    trigger::register_default(&mut registry);
    // Il motore che risolve gli strumenti per identificativo, e che riceve il
    // deposito: il `run_id` non esiste ancora qui — nasce quando la corsa parte,
    // e arriva all'azione dallo stato condiviso — mentre il deposito è già in
    // mano a chi costruisce il registro.
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(toolbox::Tools::current())
            .watched_by(watcher.clone())
            .recording_to(ledger.clone()),
    );
    // Il guardiano si attacca all'istanza che resta registrata, non a quella
    // sostituita: perciò anche questa va dopo `register_default`.
    registry.register(
        actions::SHELL_CHECK_ACTION,
        actions::ShellCheckAction::new().watched_by(watcher.clone()),
    );
    // Il passo che **non** avvia niente: descrive il lavoro e lo lascia
    // all'agente già vivo nel terminale. Va dopo `register_default` come le
    // altre due, e per la stessa ragione — il guardiano si attacca all'istanza
    // che resta registrata. Senza guardiano il mandato si vedrebbe solo a corsa
    // finita, cioè quando non serve più a nessuno.
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new().watched_by(watcher),
    );
    actions::history::register_history(&mut registry, ledger.clone());
    if let Some(ledger) = ledger {
        actions::store::register_store(&mut registry, ledger.clone());
        // Chi sta lavorando su cosa. Sta qui e non solo nella finestra perché
        // la domanda deve poterla fare **un agente**: registrata nel registro,
        // è un passo che qualunque flusso può scrivere.
        actions::presence::register_presence(&mut registry, ledger);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LE AZIONI CHE LA FINESTRA NON AVEVA.** Questa prova elenca per nome ciò
    /// che la copia del guscio si era persa per strada: senza il risolutore
    /// degli strumenti ogni passo che nomina `claude-code` invece di un percorso
    /// cadeva con `no_tool_resolver`, e senza `tool_needs` due flussi di questa
    /// casa venivano rifiutati come «azioni che il motore non conosce».
    ///
    /// Toglierne una da `default_registry` rende questa prova rossa: è l'unico
    /// modo che ho di far notare a chi verrà una riga cancellata per sbaglio.
    #[test]
    fn the_registry_carries_every_action_a_shipped_flow_can_name() {
        let registry = default_registry(None, None);
        for wanted in [
            actions::EXTERNAL_ENGINE_ACTION,
            actions::SHELL_CHECK_ACTION,
            actions::handoff::HANDED_TO_AGENT_ACTION,
            "detect_tools",
            "tool_needs",
        ] {
            assert!(
                registry.get(wanted).is_some(),
                "«{wanted}» deve stare nel registro: senza, un flusso che lo nomina non parte"
            );
        }
    }

    /// **LA RADICE ARRIVA A OGNI AZIONE, NON A QUELLE CHE SE LA VANNO A
    /// PRENDERE.** Un'azione finta registra lo `shared` che riceve: è l'unico
    /// modo di provare che il dato viaggia per costruzione e non per
    /// cortesia di chi lo legge — il guasto 28 dice cosa succede quando è il
    /// secondo caso.
    #[test]
    fn the_root_reaches_the_action_through_the_shared_state() {
        use flow::{Action, ActionError, ActionOutcome, Executor, SharedState as Shared};
        use std::sync::Mutex;

        /// Non fa niente e ricorda cosa le è arrivato.
        struct Spy(Arc<Mutex<Option<Shared>>>);
        impl Action for Spy {
            fn execute(
                &self,
                _input: &serde_json::Value,
                shared: &Shared,
            ) -> Result<ActionOutcome, ActionError> {
                *self.0.lock().expect("il registratore") = Some(shared.clone());
                Ok(ActionOutcome::Went(serde_json::Value::Null))
            }
        }

        let seen = Arc::new(Mutex::new(None));
        let mut registry = ActionRegistry::default();
        registry.register("spia", Spy(seen.clone()));

        let json = r#"{
            "id": "prova", "description": "un passo solo",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "spia", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"},
                "output_schema": {"type": "any"}
            }]},
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");
        let request = execution_request(&flow, "corsa-1", Some(Path::new("/una/radice")));

        let mut store = flow::InMemoryRecordStore::default();
        flow::InProcessExecutor
            .execute(
                &flow.graph,
                request,
                &mut store,
                &registry,
                &mut flow::SystemClock,
            )
            .expect("la corsa gira");

        let shared = seen.lock().expect("il registratore").clone().expect("il passo è girato");
        assert_eq!(
            shared.get(flow::WORKSPACE_ROOT).and_then(|root| root.as_str()),
            Some("/una/radice"),
            "la radice deve arrivare all'azione senza che l'azione la chieda"
        );
    }

    /// Senza radice non si scrive niente: **assente non è la cartella
    /// corrente**. Uno zero al posto di «non lo so» è la bugia da cui il
    /// guasto 25 è nato.
    #[test]
    fn without_a_root_nothing_is_written_into_the_shared_state() {
        let json = r#"{"id": "p", "description": "d",
            "graph": {"steps": []}, "inputs": {}}"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        let request = execution_request(&flow, "corsa-1", None);

        assert!(
            !request.shared.contains_key(flow::WORKSPACE_ROOT),
            "nessun ripiego silenzioso sulla cartella del processo"
        );
    }

    /// Senza deposito i nodi che scrivono restano fuori, quello che legge no.
    /// È la regola dichiarata sopra, e vale la pena provarla: è la differenza
    /// fra un controllo statico che crea file e uno che non tocca niente.
    #[test]
    fn without_a_ledger_the_writing_nodes_stay_out_and_the_reading_one_stays_in() {
        let registry = default_registry(None, None);
        assert!(
            registry.get("history_ask").is_some(),
            "leggere lo storico funziona anche senza deposito: la risposta è «non c'è niente»"
        );
        assert!(
            registry.get("store_put").is_none(),
            "scrivere no: senza deposito non ha dove mettere niente"
        );
    }
}
