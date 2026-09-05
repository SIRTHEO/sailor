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

/// A clock advancing by one per question, on an atomic counter: the threads of
/// a front share the clock.
struct FixedClock(std::sync::atomic::AtomicI64);

impl FixedClock {
    fn new(start: i64) -> Self {
        FixedClock(std::sync::atomic::AtomicI64::new(start))
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Result<i64, flow::FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

struct NeverRunning;

impl ProcessProbe for NeverRunning {
    fn is_running(&self, _record: &StepRecord) -> Result<bool, flow::FlowError> {
        Ok(false)
    }
}

/// Asks the kernel whether the holder written in the record is alive —
/// `kill(pid, 0)` via `/bin/kill`, which on Unix sends no signal and only asks
/// about existence. Not a fake: the same question the nightly service asks, and
/// the only way to test the "alive" branch against a real process.
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
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
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
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
}

/// An action whose effect cannot be inspected: reconciliation will always see
/// it as `Unknown`, leaving only the species to decide.
struct Opaque(StepSpecies);

impl Action for Opaque {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({})))
    }

    fn species(&self) -> StepSpecies {
        self.0
    }
}

/// Compensable and genuinely able to undo: it counts how often it has.
struct UndoesItsEffect(Arc<AtomicUsize>);

impl Action for UndoesItsEffect {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
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
        with: None,
        when: None,
        action: "opaque".to_owned(),
        max_attempts: 2,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
    }])
    .expect("valid test graph")
}

/// A step opened and never closed, as a process killed mid-run leaves it.
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
            clock: &FixedClock::new(20),
        })
        .expect("reconciliation succeeded");
    let decision = InProcessExecutor
        .decision(&graph, run_id, &store, &FixedClock::new(100))
        .expect("decision after reconciliation");
    (report, decision, store)
}

fn failure_class(store: &InMemoryRecordStore, run_id: &str) -> Option<String> {
    store
        .records(run_id)
        .expect("readable records")
        .into_iter()
        .next()
        .and_then(|record| record.failure_class)
}

/// The pair that proves the point: the same interrupted step, the same
/// uninspectable effect, the opposite outcome — decided only by what the action
/// declares about itself. Without species the first case ended up "waiting"
/// like the second, and a waiting step never becomes ready again.
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
    assert_eq!(
        undone.load(Ordering::SeqCst),
        1,
        "l'effetto va disfatto una volta sola"
    );
    assert_eq!(report.compensated, vec!["opaque"]);
    assert_eq!(report.closed_as_broke, vec!["opaque"]);
    assert_eq!(decision, Decision::Ready(vec!["opaque".to_owned()]));
    assert_eq!(
        failure_class(&store, "compensable-run").as_deref(),
        Some("compensated_then_retry")
    );
}

/// Declaring yourself compensable without being able to undo is not permission
/// to retry: the world was left half done, and there a person is needed.
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

/// A step whose holder is still alive is left alone, and no species changes
/// that: waiting costs less than cutting work in progress. The living pid here
/// is this test's own, and the contrast is the same step held by a dead pid,
/// where reconciliation does step in.
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
            clock: &FixedClock::new(20),
        })
        .expect("reconciliation succeeded");
    assert_eq!(report.still_running, vec!["opaque"]);
    assert!(report.closed_as_broke.is_empty(), "{report:?}");
    assert_eq!(
        store.records("alive-run").expect("record leggibili")[0].outcome,
        None,
        "a live process's step stays open"
    );

    // The same step with a pid no process on this machine uses: now
    // reconciliation closes it. That is the proof the probe really looks.
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
            clock: &FixedClock::new(20),
        })
        .expect("reconciliation succeeded");
    assert!(report.still_running.is_empty(), "{report:?}");
    assert_eq!(report.closed_as_broke, vec!["opaque"]);
}

/// The record beats the action: the action was rewritten after the step had
/// already started, and its new word cannot absolve an effect produced by the
/// earlier version.
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

/// Who holds the step and what species it is are written with the intent,
/// before the effect: afterwards, with the process dead, nobody writes them.
#[test]
fn the_engine_records_the_holder_and_the_species_when_it_opens_a_step() {
    let graph = opaque_graph();
    let store = InMemoryRecordStore::from_records(vec![]);
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
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &actions,
            &FixedClock::new(1),
        )
        .expect("execution succeeded");
    let record = store
        .records("opened-run")
        .expect("readable records")
        .into_iter()
        .next()
        .expect("one record written");
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
            with: None,
            when: None,
            action: "file".to_owned(),
            max_attempts: 2,
            ask_again_after_secs: None,
            retry_after_secs: None,
            phase: None,
        stops_when: None,
        decides_done: false,
        },
        Step {
            id: "next".to_owned(),
            deps: vec!["write".to_owned()],
            input_schema: ValueSchema::object(
                [("path".to_owned(), ValueSchema::String)],
                ["path".to_owned()],
            ),
            output_schema: ValueSchema::Any,
            with: None,
            when: None,
            action: "echo".to_owned(),
            max_attempts: 1,
            ask_again_after_secs: None,
            retry_after_secs: None,
            phase: None,
        stops_when: None,
        decides_done: false,
        },
    ])
    .expect("valid test graph")
}

#[test]
#[ignore = "started only as the victim process by the parent test"]
fn crash_fixture_process() {
    let Ok(record_path) = env::var(RECORD_PATH) else {
        return;
    };
    let effect_path = env::var(EFFECT_PATH).expect("effect path in the victim");
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
    let executable = env::current_exe().expect("the test binary");
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
        .expect("victim process started");

    wait_for_file(&effect_path);
    child.kill().expect("process killed mid-step");
    let status = child.wait().expect("victim process status");
    assert!(!status.success());

    let bytes = fs::read(&record_path).expect("record outlived the process");
    let records: Vec<StepRecord> = serde_json::from_slice(&bytes).expect("complete record");
    assert_eq!(records[0].outcome, None);

    let graph = recovery_graph();
    let normal = InMemoryRecordStore::from_records(records.clone());
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
                refusal: None,
                ran: None,
                ended_at: 11,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("equivalent normal close");
    let expected = InProcessExecutor
        .decision(&graph, "crashed-run", &normal, &FixedClock::new(100))
        .expect("normal decision");

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
            clock: &FixedClock::new(20),
        })
        .expect("reconciliation succeeded");
    let actual = InProcessExecutor
        .decision(&graph, "crashed-run", &recovered, &FixedClock::new(100))
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
            clock: &FixedClock::new(20),
        })
        .expect("reconciliation succeeded");
    assert_eq!(report.closed_as_broke, vec!["write"]);
    assert_eq!(
        InProcessExecutor
            .decision(&graph, "retry-run", &store, &FixedClock::new(100))
            .expect("decision after the close"),
        Decision::Ready(vec!["write".to_owned()])
    );
    cleanup(&directory, &[]);
}

fn create_test_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_nanos();
    for suffix in 0..100 {
        let path = env::temp_dir().join(format!(
            "flow-crash-{}-{nonce}-{suffix}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("temporary directory: {error}"),
        }
    }
    panic!("cannot find a free temporary name")
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "the process never wrote the effect"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn cleanup(directory: &Path, files: &[&Path]) {
    for file in files {
        fs::remove_file(file).expect("temporary file removed");
    }
    fs::remove_dir(directory).expect("temporary directory removed");
}
