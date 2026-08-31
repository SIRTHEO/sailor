//! Un flusso che ne esegue un altro, per davvero.
//!
//! **PERCHÉ QUI E NON SOLO NELLE PROVE DI MODULO.** Le funzioni del guardiano si
//! provano da sole — e sono provate — ma «il passo `subflow` esegue un altro
//! flusso» è un fatto che coinvolge il caricamento dai file, la precedenza fra
//! sorgenti, l'esecutore, il deposito e il registro delle azioni tutti insieme.
//! Una prova che ne tocca uno solo resta verde mentre il pezzo intero non
//! funziona.
//!
//! **IL DEPOSITO QUI È IN MEMORIA, E CIÒ CHE NON PROVA È DICHIARATO.** La corsa
//! figlia con `parent_run_id` scritto nel deposito vero è un fatto di
//! `crates/registry`; qui si prova che la corsa figlia **esiste con un
//! identificativo proprio**, che quell'identificativo torna nell'uscita del
//! passo del padre, e che i suoi passi si leggono sotto di esso.

use flow::subflow::{SubflowAction, SubflowHost, RunNote, SUBFLOW_ACTION};
use flow::system::FlowSource;
use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Decision, Execution, ExecutionRequest,
    Executor, FlowFile, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore,
    SharedState, Step, SystemClock, ValueSchema,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

// ── il mondo di prova ───────────────────────────────────────────────────────

/// Una cartella usa-e-getta dove scrivere i `.flow.json` della prova.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sailor-subflow-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di prova");
        Self(dir)
    }

    fn put(&self, text: &str) {
        let file: FlowFile = serde_json::from_str(text).expect("il flusso di prova è valido");
        std::fs::write(self.0.join(format!("{}.flow.json", file.id)), text).expect("scritto");
    }

    fn place(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Un'azione che scrive quello che ha ricevuto, e lo restituisce.
///
/// Serve a due prove insieme: che il figlio gira, e **con quali ingressi** —
/// che è il punto della decisione «il figlio vede solo ciò che il passo
/// dichiara».
#[derive(Default)]
struct RecordsWhatItGot {
    seen: Mutex<Vec<(Value, SharedState)>>,
}

impl Action for RecordsWhatItGot {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push((input.clone(), shared.clone()));
        Ok(ActionOutcome::Went(json!({ "echo": input.clone() })))
    }
}

/// Il ponte di prova: le sorgenti sono una cartella sola, le azioni sono quelle
/// che la prova registra, il deposito è in memoria.
///
/// **IL REGISTRO SI COSTRUISCE ALLA PRIMA CHIAMATA, COME QUELLO VERO.** È
/// l'anello che il passo `subflow` deve chiudere: gira con le stesse azioni fra
/// cui è registrato. Costruirlo prima sarebbe impossibile — conterrebbe se
/// stesso — e provarlo con un registro diverso proverebbe un'altra cosa.
struct Bench {
    dir: PathBuf,
    store: Arc<InMemoryRecordStore>,
    watcher: Arc<RecordsWhatItGot>,
    nested: OnceLock<Arc<ActionRegistry>>,
    notes: Mutex<Vec<(String, String, String, String)>>,
}

impl Bench {
    fn new(dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            dir: dir.to_path_buf(),
            store: Arc::new(InMemoryRecordStore::default()),
            watcher: Arc::new(RecordsWhatItGot::default()),
            nested: OnceLock::new(),
            notes: Mutex::new(Vec::new()),
        })
    }

    /// Il registro che contiene il passo `subflow` e l'azione di prova.
    fn registry(self: &Arc<Self>) -> ActionRegistry {
        let mut registry = ActionRegistry::default();
        registry.register(SUBFLOW_ACTION, SubflowAction::new(Arc::clone(self) as Arc<dyn SubflowHost>));
        registry.register("echo", EchoTo(Arc::clone(&self.watcher)));
        registry
    }

    /// Le intestazioni scritte: corsa figlia, padre, passo, stato.
    fn notes(&self) -> Vec<(String, String, String, String)> {
        self.notes
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }
}

/// Un guscio che manda all'unica spia della prova, così `echo` resta una riga.
struct EchoTo(Arc<RecordsWhatItGot>);

impl Action for EchoTo {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.0.execute(input, shared)
    }
}

impl SubflowHost for Bench {
    fn sources(&self) -> Vec<FlowSource> {
        vec![FlowSource {
            origin: "del progetto",
            dir: self.dir.clone(),
        }]
    }

    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        // L'anello, chiuso come lo chiude `registry::LedgerHost`.
        Ok(self
            .nested
            .get_or_init(|| {
                let mut registry = ActionRegistry::default();
                registry.register(
                    SUBFLOW_ACTION,
                    SubflowAction::new(Arc::new(BenchAgain(self.dir.clone(), Arc::clone(&self.store), Arc::clone(&self.watcher)))),
                );
                registry.register("echo", EchoTo(Arc::clone(&self.watcher)));
                Arc::new(registry)
            })
            .clone())
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::clone(&self.store) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, note: &RunNote<'_>) -> Result<(), ActionError> {
        self.notes
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push((
                note.run_id.to_owned(),
                note.parent_run_id.to_owned(),
                note.parent_step_id.to_owned(),
                note.status.to_owned(),
            ));
        Ok(())
    }
}

/// Lo stesso ponte per i livelli più profondi: stessa cartella, stesso
/// deposito, stessa spia. Esiste perché ogni livello costruisce il proprio
/// registro, esattamente come fa quello vero.
struct BenchAgain(PathBuf, Arc<InMemoryRecordStore>, Arc<RecordsWhatItGot>);

impl SubflowHost for BenchAgain {
    fn sources(&self) -> Vec<FlowSource> {
        vec![FlowSource {
            origin: "del progetto",
            dir: self.0.clone(),
        }]
    }

    fn actions(&self) -> Result<Arc<ActionRegistry>, ActionError> {
        let mut registry = ActionRegistry::default();
        registry.register(
            SUBFLOW_ACTION,
            SubflowAction::new(Arc::new(BenchAgain(
                self.0.clone(),
                Arc::clone(&self.1),
                Arc::clone(&self.2),
            ))),
        );
        registry.register("echo", EchoTo(Arc::clone(&self.2)));
        Ok(Arc::new(registry))
    }

    fn store(&self) -> Result<Arc<dyn RecordStore>, ActionError> {
        Ok(Arc::clone(&self.1) as Arc<dyn RecordStore>)
    }

    fn note_run(&self, _note: &RunNote<'_>) -> Result<(), ActionError> {
        Ok(())
    }
}

/// Il passo del padre che chiama `flow`, senza dipendenze.
fn calling_step(id: &str, calls: &str, inputs: Value) -> Step {
    Step {
        id: id.to_owned(),
        deps: Vec::new(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        with: Some(json!({ "flow": calls, "inputs": inputs })),
        when: None,
        action: SUBFLOW_ACTION.to_owned(),
        max_attempts: 1,
    }
}

/// Una chiave che il padre si porta nello stato condiviso e che il figlio non
/// deve mai vedere. Senza di lei la prova sull'eredità sarebbe vuota: si
/// asserirebbe l'assenza di qualcosa che nessuno ha mai messo.
const PARENT_ONLY: &str = "segreto-del-padre";

/// Fa girare un grafo di un passo solo che chiama `calls`.
fn run_calling(
    bench: &Arc<Bench>,
    calls: &str,
    inputs: Value,
    cap: Option<i64>,
) -> Execution {
    let graph = Graph::new(vec![calling_step("chiamata", calls, inputs)]).expect("grafo valido");
    let registry = bench.registry();
    let mut shared = SharedState::new();
    shared.insert(PARENT_ONLY.to_owned(), json!("non deve arrivare al figlio"));
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-del-padre".to_owned(),
                root_inputs: Default::default(),
                gates: Vec::new(),
                shared,
                spend_cap_micros: cap,
            },
            bench.store.as_ref(),
            &registry,
            &SystemClock,
        )
        .expect("l'esecuzione non è un guasto del motore")
}

/// Il passo del padre come lo ha chiuso il deposito.
fn parent_step(bench: &Arc<Bench>) -> flow::StepRecord {
    bench
        .store
        .records("corsa-del-padre")
        .expect("leggere i passi")
        .into_iter()
        .find(|record| record.step_id == "chiamata")
        .expect("il passo c'è")
}

// ── i flussi di prova ───────────────────────────────────────────────────────

const LEAF: &str = r#"{
  "id": "foglia",
  "description": "un flusso interno di un passo solo",
  "graph": { "steps": [{
    "id": "riporta", "deps": [], "action": "echo", "max_attempts": 1, "when": null,
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": { "riporta": { "scritto-nel-file": true } }
}"#;

const HERE: &str = r#"{
  "id": "andata",
  "description": "chiama ritorno",
  "graph": { "steps": [{
    "id": "vai", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
    "with": { "flow": "ritorno" },
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": {}
}"#;

const BACK: &str = r#"{
  "id": "ritorno",
  "description": "richiama andata: è l'anello",
  "graph": { "steps": [{
    "id": "torna", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
    "with": { "flow": "andata" },
    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
  }] },
  "inputs": {}
}"#;

// ── le prove ────────────────────────────────────────────────────────────────

/// **IL FATTO CENTRALE: UN PASSO ESEGUE UN ALTRO FLUSSO.**
///
/// Il figlio gira, la sua uscita torna nell'uscita del passo, e il passo del
/// padre porta il `run_id` del figlio — che è la metà risalibile della
/// decisione 4.
#[test]
fn a_step_runs_another_flow_and_carries_back_its_output() {
    let scratch = Scratch::new("esegue");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    let execution = run_calling(&bench, "foglia", json!({}), None);

    assert_eq!(execution.decisions.last(), Some(&Decision::Complete));
    let record = parent_step(&bench);
    assert_eq!(record.outcome, Some(Outcome::Went));
    let output = record.output.expect("il passo ha un'uscita");
    assert_eq!(output["flow"], "foglia");
    assert_eq!(output["origin"], "del progetto");
    assert_eq!(output["status"], "complete");
    assert_eq!(
        output["outputs"]["riporta"]["echo"]["scritto-nel-file"],
        json!(true),
        "l'uscita del passo terminale del figlio è l'uscita del passo: {output}"
    );

    let child = output["run_id"].as_str().expect("il figlio ha una corsa");
    assert!(
        child.starts_with("corsa-del-padre::chiamata::"),
        "la corsa figlia si risale dal nome: {child}"
    );
    assert_eq!(
        bench.store.records(child).expect("leggere").len(),
        1,
        "e i suoi passi stanno sotto di lei, non sotto il padre"
    );
}

/// **LA CORSA FIGLIA È UNA CORSA, CON IL PADRE SCRITTO ACCANTO.**
///
/// Aperta `running` e chiusa `complete`, con il passo che l'ha chiamata. Senza
/// questa, «risalibile» sarebbe una parola nel commento.
#[test]
fn the_child_run_names_the_run_and_the_step_that_called_it() {
    let scratch = Scratch::new("intestazione");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "foglia", json!({}), None);

    let notes = bench.notes();
    assert_eq!(notes.len(), 2, "una all'apertura e una alla chiusura: {notes:?}");
    assert_eq!(notes[0].1, "corsa-del-padre");
    assert_eq!(notes[0].2, "chiamata");
    assert_eq!(notes[0].3, "running");
    assert_eq!(notes[1].3, "complete");
    assert_eq!(notes[0].0, notes[1].0, "è la stessa corsa, aperta e chiusa");
}

/// **IL FIGLIO VEDE CIÒ CHE IL PASSO DICHIARA, E NON LO STATO DEL PADRE.**
///
/// Gli ingressi del passo vincono su quelli scritti nel file del figlio, e la
/// mappa condivisa del padre non arriva: se arrivasse, nessuno potrebbe più
/// dire da dove viene un valore.
#[test]
fn the_child_gets_the_declared_inputs_and_not_the_parent_state() {
    let scratch = Scratch::new("ingressi");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(
        &bench,
        "foglia",
        json!({ "riporta": { "dal-passo": "questo" } }),
        None,
    );

    let seen = bench.watcher.seen.lock().unwrap_or_else(|held| held.into_inner());
    let (input, shared) = seen.first().expect("il figlio ha girato");
    assert_eq!(input["dal-passo"], "questo", "l'ingresso del passo vince");
    assert!(
        input.get("scritto-nel-file").is_none(),
        "e sostituisce quello del file per quella chiave: {input}"
    );
    assert_eq!(
        shared.get(flow::CURRENT_RUN).and_then(Value::as_str).map(|run| run.contains("corsa-del-padre")),
        Some(true),
        "la corsa del figlio porta il padre nel nome"
    );
    assert!(
        !shared.contains_key(PARENT_ONLY),
        "niente dello stato condiviso del padre passa al figlio: {shared:?}"
    );
}

/// **DUE FLUSSI CHE SI CHIAMANO A VICENDA SI FERMANO, E L'ERRORE NOMINA LA
/// CATENA.**
///
/// È il caso che il controllo dei cicli del grafo **non può** vedere: ciascuno
/// dei due file, da solo, è un grafo perfettamente aciclico. L'anello esiste
/// solo fra i due, e senza qualcuno che lo cerchi la corsa girerebbe finché la
/// pila non finisce.
///
/// **E DEVE DIRE CHI CHIAMA CHI.** «Ciclo rilevato» non si può riparare: chi
/// legge deve poter togliere l'arco, e per toglierlo deve vederlo.
#[test]
fn two_flows_that_call_each_other_stop_with_the_chain_written_out() {
    let scratch = Scratch::new("anello");
    scratch.put(HERE);
    scratch.put(BACK);
    let bench = Bench::new(scratch.place());

    let execution = run_calling(&bench, "andata", json!({}), None);

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Failed(_))),
        "la corsa del padre si ferma: {:?}",
        execution.decisions.last()
    );
    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("subflow_cycle"));
    let said = record.said.unwrap_or_default();
    assert!(
        said.contains("andata → ritorno → andata"),
        "l'errore deve nominare la catena, non dire soltanto «ciclo»: {said}"
    );
}

/// **UN FLUSSO CHE CHIAMA SE STESSO È LO STESSO GUASTO, PIÙ CORTO.**
#[test]
fn a_flow_that_calls_itself_names_itself_twice() {
    let scratch = Scratch::new("solitario");
    scratch.put(
        r#"{
      "id": "solitario",
      "description": "chiama se stesso",
      "graph": { "steps": [{
        "id": "ancora", "deps": [], "action": "subflow", "max_attempts": 1, "when": null,
        "with": { "flow": "solitario" },
        "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
      }] },
      "inputs": {}
    }"#,
    );
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "solitario", json!({}), None);

    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("subflow_cycle"));
    assert!(
        record.said.unwrap_or_default().contains("solitario → solitario"),
        "l'anello più corto si nomina come gli altri"
    );
}

/// **UN NOME CHE NESSUNA SORGENTE CONOSCE NON È UN ANELLO.**
///
/// Senza questa prova, un guardiano che dicesse «ciclo» a ogni chiamata
/// resterebbe verde su quelle sopra. E l'errore deve dire **dove ha guardato**:
/// un flusso che manca si ripara scrivendolo nel posto giusto.
#[test]
fn a_call_to_a_flow_that_does_not_exist_says_where_it_looked() {
    let scratch = Scratch::new("assente");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "mai-scritto", json!({}), None);

    let record = parent_step(&bench);
    assert_eq!(record.failure_class.as_deref(), Some("unknown_subflow"));
    let said = record.said.unwrap_or_default();
    assert!(said.contains("mai-scritto"), "dice quale flusso: {said}");
    assert!(said.contains("del progetto"), "e dove ha guardato: {said}");
}

/// **IL TETTO DEL PADRE VALE ANCHE PER IL FIGLIO.**
///
/// Il figlio non dichiara nessun tetto: se ereditasse «nessun limite»,
/// spostare la spesa dentro un sotto-flusso annullerebbe il tetto di chiunque.
/// Qui il padre ha zero da spendere, e il figlio si ferma prima del primo
/// passo — cioè riceve il residuo, non l'assenza.
#[test]
fn the_child_inherits_what_is_left_of_the_parent_cap() {
    let scratch = Scratch::new("tetto");
    scratch.put(LEAF);
    let bench = Bench::new(scratch.place());

    run_calling(&bench, "foglia", json!({}), Some(0));

    // Con tetto zero il padre non apre nemmeno il proprio passo: è il tetto che
    // lavora al livello di sopra. La prova vera è la riga sotto.
    let with_room = Bench::new(scratch.place());
    run_calling(&with_room, "foglia", json!({}), Some(1_000_000));
    let record = parent_step(&with_room);
    assert_eq!(
        record.outcome,
        Some(Outcome::Went),
        "con margine il figlio gira: {record:?}"
    );

    let seen = with_room.watcher.seen.lock().unwrap_or_else(|held| held.into_inner());
    let (_, shared) = seen.first().expect("il figlio ha girato");
    assert_eq!(
        shared.get(flow::CURRENT_CAP).and_then(Value::as_i64),
        Some(1_000_000),
        "e gira sotto il tetto che gli resta dal padre, non senza tetto: {shared:?}"
    );
}

/// **IL PASSO NOMINA I CAMPI CHE NON CONOSCE**, che è come `flow check` scopre
/// un refuso prima che costi una chiamata a pagamento.
#[test]
fn a_field_the_step_does_not_know_is_named() {
    let scratch = Scratch::new("campi");
    let bench = Bench::new(scratch.place());
    let registry = bench.registry();
    let step = registry.get(SUBFLOW_ACTION).expect("registrata");

    assert_eq!(
        step.unknown_fields(&json!({ "flow": "foglia", "inputs": {}, "flusso": "foglia" })),
        vec!["flusso".to_owned()]
    );
    assert!(step
        .unknown_fields(&json!({ "flow": "foglia", "inputs": {} }))
        .is_empty());
}
