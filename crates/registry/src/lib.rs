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
mod subflow_host;

pub use run_record::{
    execution_status, record_child_run, record_flow_run, stopped_by_cap, why_it_stopped, FlowRun,
};
pub use subflow_host::LedgerHost;

use std::sync::Arc;

use flow::ActionRegistry;
use ledger::Ledger;

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
    actions::history::register_history(&mut registry, ledger.clone());
    // UN FLUSSO CHE NE ESEGUE UN ALTRO. Si registra **anche senza deposito**,
    // e la differenza è la stessa dichiarata sopra: `flow check` deve poter
    // dire che un passo `subflow` nomina un'azione che esiste, senza aprire
    // niente. A eseguire, invece, senza deposito si rifiuta dicendolo — una
    // corsa figlia che nessuno può ricollegare al passo che l'ha chiesta è
    // proprio l'opacità per cui questo passo è stato costruito.
    registry.register(
        flow::subflow::SUBFLOW_ACTION,
        flow::subflow::SubflowAction::new(Arc::new(LedgerHost::new(
            ledger.clone(),
            watcher,
        ))),
    );
    if let Some(ledger) = ledger {
        actions::store::register_store(&mut registry, ledger);
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
            "detect_tools",
            "tool_needs",
            // La finestra offre il nodo `subflow` da sempre; fino al
            // 31/08/2026 il motore non lo conosceva, e un flusso disegnato con
            // quel nodo veniva rifiutato come «azione che non conosco».
            flow::subflow::SUBFLOW_ACTION,
        ] {
            assert!(
                registry.get(wanted).is_some(),
                "«{wanted}» deve stare nel registro: senza, un flusso che lo nomina non parte"
            );
        }
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

    /// **`subflow` C'È ANCHE SENZA DEPOSITO, MA NON ESEGUE.**
    ///
    /// Le due metà stanno insieme apposta. Deve **esserci**, o `flow check` —
    /// che un deposito non lo apre — direbbe che un passo `subflow` nomina
    /// un'azione sconosciuta, cioè rifiuterebbe un flusso valido. E deve
    /// **rifiutarsi di eseguire**, perché una corsa figlia che non finisce nel
    /// deposito non è risalibile dal passo che l'ha chiamata: sarebbe
    /// esattamente il lavoro-che-sparisce-dentro-un-altro per cui questo passo
    /// è stato costruito.
    #[test]
    fn without_a_ledger_the_subflow_step_is_registered_but_refuses_to_run() {
        let registry = default_registry(None, None);
        let step = registry
            .get(flow::subflow::SUBFLOW_ACTION)
            .expect("registrata anche senza deposito");

        let mut shared = flow::SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), "una-corsa".into());
        shared.insert(flow::CURRENT_STEP.to_owned(), "un-passo".into());
        let refused = step
            .execute(&serde_json::json!({ "flow": "qualunque" }), &shared)
            .expect_err("senza deposito non deve eseguire");

        assert_eq!(refused.class, "no_ledger");
        assert!(
            refused.said.contains("risalibile"),
            "e dice perché, non solo che non può: {}",
            refused.said
        );
    }

    /// La corsa figlia porta il padre nel deposito, non solo nel proprio nome.
    /// Senza `parent_run_id` scritto, «risalire» vorrebbe dire indovinare da una
    /// stringa — e chi ha lanciato il figlio si legge in `started_by`.
    #[test]
    fn a_child_run_carries_the_parent_and_the_step_that_started_it() {
        let dir = std::env::temp_dir().join(format!("sailor-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("deposito di prova");
        let flow: flow::FlowFile = serde_json::from_str(
            r#"{"id":"figlio","description":"x","graph":{"steps":[]},"inputs":{}}"#,
        )
        .expect("flusso valido");

        record_child_run(
            &ledger,
            &flow,
            FlowRun {
                run_id: "corsa-figlia",
                status: "complete",
                started_at: 1,
                ended_at: Some(2),
                error: None,
                started_by: "subflow chiamata",
            },
            "corsa-del-padre",
        )
        .expect("registrata");

        // **SI LEGGE DALLA PROIEZIONE, PERCHÉ IL DEPOSITO NON HA UN LETTORE PER
        // QUESTA COLONNA.** `FinishedRun` non porta `parent_run_id`: la colonna
        // esisteva e nessuno la riempiva, quindi nessuno l'ha mai riletta. Il
        // dump la espone per posizione — `run_id, kind, entity, parent_run_id,
        // started_by, …` — ed è l'unica strada che non chiede di modificare il
        // deposito.
        let dump = ledger.projection_dump().expect("proiezione");
        let child = dump["runs"]
            .as_array()
            .expect("le corse")
            .iter()
            .find(|row| row[0] == "corsa-figlia")
            .expect("la corsa figlia c'è");
        assert_eq!(child[2], "figlio", "il flusso eseguito");
        assert_eq!(child[3], "corsa-del-padre", "la corsa che l'ha chiamata");
        assert_eq!(child[4], "subflow chiamata", "e il passo che l'ha aperta");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
