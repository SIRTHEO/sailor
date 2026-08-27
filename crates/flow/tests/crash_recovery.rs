use flow::{
    Action, ActionError, ActionOutcome, ActionRegistry, Clock, Completion, Decision, EffectStatus,
    Graph, InMemoryRecordStore, InProcessExecutor, Outcome, ProcessProbe, ReconciliationRequest,
    RecordStore, SharedState, Step, StepRecord, ValueSchema,
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
