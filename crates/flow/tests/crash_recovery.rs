use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Completion, Decision, EffectStatus,
    Executor, Graph, InMemoryRecordStore, InProcessExecutor, Outcome, ProcessProbe,
    ReconciliationRequest, RecordStore, SharedState, Step, StepRecord, StepSpecies, ValueSchema,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RECORD_PATH: &str = "FLOW_CRASH_RECORD_PATH";
const EFFECT_PATH: &str = "FLOW_CRASH_EFFECT_PATH";

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now(&mut self) -> Result<i64, flow::FlowError> {
        self.0 += 1;
        Ok(self.0)
    }
}

struct NeverRunning;

impl ProcessProbe for NeverRunning {
    fn is_running(&self, _record: &StepRecord) -> Result<bool, flow::FlowError> {
        Ok(false)
    }
}

/// Chiede al kernel se il detentore scritto nel record è vivo — `kill(pid, 0)`
/// via `/bin/kill`, che su Unix non manda nessun segnale e chiede solo
/// l'esistenza. Non è una finzione: è la stessa domanda che fa il servizio
/// notturno, ed è l'unico modo di provare il ramo «vivo» con un processo vero.
struct AsksTheKernel;

impl ProcessProbe for AsksTheKernel {
    fn is_running(&self, record: &StepRecord) -> Result<bool, flow::FlowError> {
        let Some(pid) = record.held_by_pid else {
            return Ok(false);
        };
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }
}

struct FileEffect {
    path: PathBuf,
    executions: Arc<AtomicUsize>,
}

impl Action for FileEffect {
    fn execute(
        &self,
        _input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        fs::write(&self.path, b"landed")
            .map_err(|error| ActionError::new("write_failed", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({"path": self.path})))
    }

    fn inspect_effect(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        if self.path.is_file() {
            Ok(EffectStatus::Applied(json!({"path": self.path})))
        } else {
            Ok(EffectStatus::NotApplied)
        }
    }
}

struct Echo;

impl Action for Echo {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

/// Un'azione il cui effetto non si può ispezionare: la riconciliazione la
/// vedrà sempre come `Unknown`, e a decidere resterà solo la specie.
struct Opaque(StepSpecies);

impl Action for Opaque {
    fn execute(
        &self,
        _input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({})))
    }

    fn species(&self) -> StepSpecies {
        self.0
    }
}

/// Compensabile e capace di disfare davvero: conta quante volte l'ha fatto.
struct UndoesItsEffect(Arc<AtomicUsize>);

impl Action for UndoesItsEffect {
    fn execute(
        &self,
        _input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({})))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Compensable
    }

    fn compensate(&self, _record: &StepRecord, _shared: &SharedState) -> Result<(), ActionError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn opaque_graph() -> Graph {
    Graph::new(vec![Step {
        id: "opaque".to_owned(),
        deps: vec![],
        input_schema: ValueSchema::Null,
        output_schema: ValueSchema::Any,
        when: None,
        action: "opaque".to_owned(),
        max_attempts: 2,
    }])
    .expect("grafo di prova valido")
}

/// Un passo aperto e mai chiuso, come lo lascia un processo ucciso a metà.
fn interrupted_step(run_id: &str, species: Option<StepSpecies>) -> StepRecord {
    let mut record = StepRecord::started(run_id, "opaque", 1, 1, vec![], Value::Null, vec![], 10);
    record.species = species;
    record
}

fn reconcile_opaque(
    run_id: &str,
    record: StepRecord,
    action: impl Action + 'static,
) -> (flow::Reconciliation, Decision, InMemoryRecordStore) {
    let graph = opaque_graph();
    let mut store = InMemoryRecordStore::from_records(vec![record]);
    let mut actions = ActionRegistry::default();
    actions.register("opaque", action);
    let report = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph,
            run_id,
            store: &mut store,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &NeverRunning,
            clock: &mut FixedClock(20),
        })
        .expect("riconciliazione riuscita");
    let decision = InProcessExecutor
        .decision(&graph, run_id, &store)
        .expect("decisione dopo la riconciliazione");
    (report, decision, store)
}

fn failure_class(store: &InMemoryRecordStore, run_id: &str) -> Option<String> {
    store
        .records(run_id)
        .expect("record leggibili")
        .into_iter()
        .next()
        .and_then(|record| record.failure_class)
}

/// La coppia che dimostra il punto: stesso passo interrotto, stesso effetto
/// non ispezionabile, esito opposto — deciso solo da ciò che l'azione
/// dichiara di sé. Senza la specie il primo caso finiva «in attesa» come il
/// secondo, e un passo in attesa non torna mai pronto.
#[test]
fn an_opaque_repeatable_step_is_reopened_instead_of_waiting_for_a_person() {
    let (report, decision, store) = reconcile_opaque(
        "repeatable-run",
        interrupted_step("repeatable-run", None),
        Opaque(StepSpecies::Repeatable),
    );
    assert_eq!(report.closed_as_broke, vec!["opaque"]);
    assert!(report.closed_as_waiting.is_empty());
    assert!(report.compensated.is_empty());
    assert_eq!(decision, Decision::Ready(vec!["opaque".to_owned()]));
    assert_eq!(
        failure_class(&store, "repeatable-run").as_deref(),
        Some("repeatable_after_unknown_effect")
    );
}

#[test]
fn an_opaque_step_that_declares_nothing_is_handed_to_a_person() {
    let (report, decision, store) = reconcile_opaque(
        "silent-run",
        interrupted_step("silent-run", None),
        Echo, // non dichiara la propria specie: vale il valore di difesa
    );
    assert_eq!(report.closed_as_waiting, vec!["opaque"]);
    assert!(report.closed_as_broke.is_empty());
    assert_eq!(decision, Decision::Waiting(vec!["opaque".to_owned()]));
    assert_eq!(
        failure_class(&store, "silent-run").as_deref(),
        Some("effect_unknown")
    );
}

#[test]
fn a_compensable_step_undoes_its_effect_before_being_reopened() {
    let undone = Arc::new(AtomicUsize::new(0));
    let (report, decision, store) = reconcile_opaque(
        "compensable-run",
        interrupted_step("compensable-run", None),
        UndoesItsEffect(Arc::clone(&undone)),
    );
    assert_eq!(undone.load(Ordering::SeqCst), 1, "l'effetto va disfatto una volta sola");
    assert_eq!(report.compensated, vec!["opaque"]);
    assert_eq!(report.closed_as_broke, vec!["opaque"]);
    assert_eq!(decision, Decision::Ready(vec!["opaque".to_owned()]));
    assert_eq!(
        failure_class(&store, "compensable-run").as_deref(),
        Some("compensated_then_retry")
    );
}

/// Dichiararsi compensabile senza saper disfare non è un permesso di
/// ritentare: il mondo è rimasto a metà, e lì serve una persona.
#[test]
fn a_compensable_step_that_cannot_undo_waits_for_a_person() {
    let (report, decision, store) = reconcile_opaque(
        "broken-compensation-run",
        interrupted_step("broken-compensation-run", None),
        Opaque(StepSpecies::Compensable),
    );
    assert!(report.compensated.is_empty());
    assert_eq!(report.closed_as_waiting, vec!["opaque"]);
    assert_eq!(decision, Decision::Waiting(vec!["opaque".to_owned()]));
    assert_eq!(
        failure_class(&store, "broken-compensation-run").as_deref(),
        Some("no_compensation")
    );
}

/// Un passo il cui detentore è ancora vivo non si tocca, e nessuna specie
/// cambia questo: aspettare costa meno che troncare un lavoro in corso. Il pid
/// vivo qui è quello di questa prova, e il confronto è con lo stesso passo
/// tenuto da un pid morto — dove invece la riconciliazione interviene.
#[test]
fn a_step_held_by_a_living_process_is_left_alone() {
    let mut alive = interrupted_step("alive-run", Some(StepSpecies::Repeatable));
    alive.held_by_pid = Some(std::process::id());
    let graph = opaque_graph();
    let mut store = InMemoryRecordStore::from_records(vec![alive]);
    let mut actions = ActionRegistry::default();
    actions.register("opaque", Opaque(StepSpecies::Repeatable));
    let report = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph,
            run_id: "alive-run",
            store: &mut store,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &AsksTheKernel,
            clock: &mut FixedClock(20),
        })
        .expect("riconciliazione riuscita");
    assert_eq!(report.still_running, vec!["opaque"]);
    assert!(report.closed_as_broke.is_empty(), "{report:?}");
    assert_eq!(
        store.records("alive-run").expect("record leggibili")[0].outcome,
        None,
        "il passo di un processo vivo resta aperto"
    );

    // Lo stesso passo, con un pid che nessun processo su questa macchina usa:
    // ora la riconciliazione lo chiude. È la prova che il probe guarda davvero.
    let mut dead = interrupted_step("dead-run", Some(StepSpecies::Repeatable));
    dead.held_by_pid = Some(999_999);
    let mut store = InMemoryRecordStore::from_records(vec![dead]);
    let report = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph,
            run_id: "dead-run",
            store: &mut store,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &AsksTheKernel,
            clock: &mut FixedClock(20),
        })
        .expect("riconciliazione riuscita");
    assert!(report.still_running.is_empty(), "{report:?}");
    assert_eq!(report.closed_as_broke, vec!["opaque"]);
}

/// Il record vince sull'azione: l'azione è stata riscritta dopo che il passo
/// era già partito, e la sua parola nuova non può assolvere un effetto
/// prodotto dalla versione di prima.
#[test]
fn the_species_written_when_the_step_started_beats_the_action_of_today() {
    let (report, decision, _) = reconcile_opaque(
        "frozen-run",
        interrupted_step("frozen-run", Some(StepSpecies::HandToHuman)),
        Opaque(StepSpecies::Repeatable),
    );
    assert_eq!(report.closed_as_waiting, vec!["opaque"]);
    assert_eq!(decision, Decision::Waiting(vec!["opaque".to_owned()]));
}

/// Chi tiene il passo e di che specie è si scrivono con l'intenzione, prima
/// dell'effetto: dopo, a processo morto, non li scrive più nessuno.
#[test]
fn the_engine_records_the_holder_and_the_species_when_it_opens_a_step() {
    let graph = opaque_graph();
    let mut store = InMemoryRecordStore::from_records(vec![]);
    let mut actions = ActionRegistry::default();
    actions.register("opaque", Opaque(StepSpecies::Repeatable));
    InProcessExecutor
        .execute(
            &graph,
            flow::ExecutionRequest {
                run_id: "opened-run".to_owned(),
                root_inputs: BTreeMap::new(),
                gates: vec![],
                shared: SharedState::new(),
            },
            &mut store,
            &actions,
            &mut FixedClock(1),
        )
        .expect("esecuzione riuscita");
    let record = store
        .records("opened-run")
        .expect("record leggibili")
        .into_iter()
        .next()
        .expect("un record scritto");
    assert_eq!(record.held_by_pid, Some(std::process::id()));
    assert_eq!(record.species, Some(StepSpecies::Repeatable));
}

fn recovery_graph() -> Graph {
    Graph::new(vec![
        Step {
            id: "write".to_owned(),
            deps: vec![],
            input_schema: ValueSchema::Null,
            output_schema: ValueSchema::object(
                [("path".to_owned(), ValueSchema::String)],
                ["path".to_owned()],
            ),
            when: None,
            action: "file".to_owned(),
            max_attempts: 2,
        },
        Step {
            id: "next".to_owned(),
            deps: vec!["write".to_owned()],
            input_schema: ValueSchema::object(
                [("path".to_owned(), ValueSchema::String)],
                ["path".to_owned()],
            ),
            output_schema: ValueSchema::Any,
            when: None,
            action: "echo".to_owned(),
            max_attempts: 1,
        },
    ])
    .expect("grafo di prova valido")
}

#[test]
#[ignore = "avviato soltanto come processo vittima dalla prova padre"]
fn crash_fixture_process() {
    let Ok(record_path) = env::var(RECORD_PATH) else {
        return;
    };
    let effect_path = env::var(EFFECT_PATH).expect("percorso dell'effetto nella vittima");
    let record = StepRecord::started(
        "crashed-run",
        "write",
        1,
        1,
        vec![],
        Value::Null,
        vec!["filesystem".to_owned()],
        10,
    );
    let bytes = serde_json::to_vec(&vec![record]).expect("record serializzabile");
    fs::write(record_path, bytes).expect("intenzione durevole prima dell'effetto");
    fs::write(effect_path, b"landed").expect("effetto atterrato");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn killed_process_reconstructs_the_same_next_decision_without_replaying_effect() {
    let directory = create_test_directory();
    let record_path = directory.join("records.json");
    let effect_path = directory.join("effect.txt");
    let executable = env::current_exe().expect("binario della prova");
    let mut child = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "crash_fixture_process",
            "--test-threads=1",
        ])
        .env(RECORD_PATH, &record_path)
        .env(EFFECT_PATH, &effect_path)
        .spawn()
        .expect("processo vittima avviato");

    wait_for_file(&effect_path);
    child.kill().expect("processo ucciso a metà passo");
    let status = child.wait().expect("stato del processo vittima");
    assert!(!status.success());

    let bytes = fs::read(&record_path).expect("record sopravvissuto al processo");
    let records: Vec<StepRecord> = serde_json::from_slice(&bytes).expect("record completo");
    assert_eq!(records[0].outcome, None);

    let graph = recovery_graph();
    let mut normal = InMemoryRecordStore::from_records(records.clone());
    normal
        .close(
            "crashed-run",
            "write",
            1,
            1,
            Completion {
                outcome: Outcome::Went,
                output: Some(json!({"path": effect_path})),
                said: None,
                failure_class: None,
                ended_at: 11,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("chiusura normale equivalente");
    let expected = InProcessExecutor
        .decision(&graph, "crashed-run", &normal)
        .expect("decisione normale");

    let executions = Arc::new(AtomicUsize::new(0));
    let mut actions = ActionRegistry::default();
    actions.register(
        "file",
        FileEffect {
            path: effect_path.clone(),
            executions: Arc::clone(&executions),
        },
    );
    actions.register("echo", Echo);
    let mut recovered = InMemoryRecordStore::from_records(records);
    let report = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph,
            run_id: "crashed-run",
            store: &mut recovered,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &NeverRunning,
            clock: &mut FixedClock(20),
        })
        .expect("riconciliazione riuscita");
    let actual = InProcessExecutor
        .decision(&graph, "crashed-run", &recovered)
        .expect("decisione ricostruita");

    assert_eq!(report.closed_as_went, vec!["write"]);
    assert_eq!(actual, expected);
    assert_eq!(actual, Decision::Ready(vec!["next".to_owned()]));
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    cleanup(&directory, &[&record_path, &effect_path]);
}

#[test]
fn absent_process_without_landed_effect_closes_and_retries_itself() {
    let directory = create_test_directory();
    let effect_path = directory.join("missing.txt");
    let graph = recovery_graph();
    let record = StepRecord::started("retry-run", "write", 1, 1, vec![], Value::Null, vec![], 10);
    let mut store = InMemoryRecordStore::from_records(vec![record]);
    let mut actions = ActionRegistry::default();
    actions.register(
        "file",
        FileEffect {
            path: effect_path,
            executions: Arc::new(AtomicUsize::new(0)),
        },
    );
    actions.register("echo", Echo);
    let report = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph,
            run_id: "retry-run",
            store: &mut store,
            actions: &actions,
            shared: &BTreeMap::new(),
            processes: &NeverRunning,
            clock: &mut FixedClock(20),
        })
        .expect("riconciliazione riuscita");
    assert_eq!(report.closed_as_broke, vec!["write"]);
    assert_eq!(
        InProcessExecutor
            .decision(&graph, "retry-run", &store)
            .expect("decisione dopo la chiusura"),
        Decision::Ready(vec!["write".to_owned()])
    );
    cleanup(&directory, &[]);
}

fn create_test_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("orologio dopo epoch")
        .as_nanos();
    for suffix in 0..100 {
        let path = env::temp_dir().join(format!(
            "flow-crash-{}-{nonce}-{suffix}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cartella temporanea: {error}"),
        }
    }
    panic!("impossibile trovare un nome temporaneo libero")
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "il processo non ha scritto l'effetto"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn cleanup(directory: &Path, files: &[&Path]) {
    for file in files {
        fs::remove_file(file).expect("file temporaneo rimosso");
    }
    fs::remove_dir(directory).expect("cartella temporanea rimossa");
}
