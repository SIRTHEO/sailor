use flow::{
    ActionRegistry, Clock, EffectStatus, ExecutionRequest, Executor, Graph, InMemoryRecordStore,
    InProcessExecutor, Outcome, ProcessProbe, ReconciliationRequest, SharedState, Step, StepRecord,
    ValueSchema,
};
use launcher::{hold_process_lock, CommandSpec, EffectInspector, FileLockProbe, ProcessExecutor};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCK_PATH_ENV: &str = "LAUNCHER_TEST_LOCK_PATH";
const READY_PATH_ENV: &str = "LAUNCHER_TEST_READY_PATH";

struct FixedClock(i64);

impl Clock for FixedClock {
    fn now(&mut self) -> Result<i64, flow::FlowError> {
        self.0 += 1;
        Ok(self.0)
    }
}

struct NotApplied;

impl EffectInspector for NotApplied {
    fn inspect(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, flow::ActionError> {
        Ok(EffectStatus::NotApplied)
    }
}

fn graph() -> Graph {
    Graph::new(vec![Step {
        id: "command".to_owned(),
        deps: vec![],
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
        when: None,
        action: "external".to_owned(),
        max_attempts: 1,
    }])
    .expect("grafo di prova valido")
}

fn execute(
    directory: &Path,
    script: &str,
    limit: usize,
    environment: BTreeMap<std::ffi::OsString, std::ffi::OsString>,
) -> InMemoryRecordStore {
    let mut executor = ProcessExecutor::new(directory.join("locks"));
    let mut spec = CommandSpec::new("/bin/sh", directory, Duration::from_millis(150), limit);
    spec.arguments = vec!["-c".into(), script.into()];
    spec.environment = environment;
    executor.register("external", spec);
    let mut store = InMemoryRecordStore::default();
    executor
        .execute(
            &graph(),
            ExecutionRequest {
                run_id: "run".to_owned(),
                root_inputs: BTreeMap::from([("command".to_owned(), Value::Null)]),
                gates: vec!["processes".to_owned()],
                shared: SharedState::new(),
            },
            &mut store,
            &ActionRegistry::default(),
            &mut FixedClock(10),
        )
        .expect("esecuzione terminata");
    store
}

#[test]
fn timeout_kills_a_real_non_terminating_command_and_closes_its_record() {
    let directory = create_test_directory();
    let started = Instant::now();
    let store = execute(
        &directory,
        "trap '' TERM; while :; do sleep 1; done",
        128,
        BTreeMap::new(),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let record = &store.all()[0];
    assert_eq!(record.outcome, Some(Outcome::Broke));
    assert_eq!(record.failure_class.as_deref(), Some("timeout"));
    assert!(record.ended_at.is_some());
    cleanup_directory(&directory);
}

#[test]
fn timeout_kills_the_process_group_without_leaving_the_spawned_child() {
    let directory = create_test_directory();
    let pid_path = directory.join("child.pid");
    let mut environment = BTreeMap::new();
    environment.insert("PID_FILE".into(), pid_path.as_os_str().to_owned());
    let script = "trap '' TERM; eval \"sleep 30 </dev/null >/dev/null 2>&1 $SAILOR_OUTPUT_FD>&- &\"; child=$!; echo $child > \"$PID_FILE\"; eval \"printf null >&$SAILOR_OUTPUT_FD\"; wait";
    let store = execute(&directory, script, 128, environment);
    assert_eq!(store.all()[0].failure_class.as_deref(), Some("timeout"));
    let pid: i32 = fs::read_to_string(&pid_path)
        .expect("pid del nipote scritto")
        .trim()
        .parse()
        .expect("pid numerico");
    let dead = wait_until_dead(pid);
    if !dead {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(dead, "il processo discendente {pid} è rimasto vivo");
    cleanup_directory(&directory);
}

#[test]
fn file_lock_tracks_life_and_flow_reconciles_after_its_release() {
    let directory = create_test_directory();
    let ready_path = directory.join("ready");
    let launched_record = StepRecord::started(
        "run",
        "command",
        1,
        1,
        vec![],
        Value::Null,
        vec!["processes".to_owned()],
        11,
    );
    let launched_probe = FileLockProbe::new(directory.join("launched-locks"));
    let execution_directory = directory.clone();
    let execution = thread::spawn(move || {
        let mut executor = ProcessExecutor::new(execution_directory.join("launched-locks"));
        let mut spec = CommandSpec::new(
            "/bin/sh",
            &execution_directory,
            Duration::from_millis(400),
            128,
        );
        spec.arguments = vec![
            "-c".into(),
            "trap '' TERM; while :; do sleep 1; done".into(),
        ];
        executor.register("external", spec);
        let mut store = InMemoryRecordStore::default();
        executor
            .execute(
                &graph(),
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: BTreeMap::from([("command".to_owned(), Value::Null)]),
                    gates: vec!["processes".to_owned()],
                    shared: SharedState::new(),
                },
                &mut store,
                &ActionRegistry::default(),
                &mut FixedClock(10),
            )
            .expect("processo reale eseguito");
    });
    let lock_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if launched_probe
            .is_running(&launched_record)
            .expect("lock del processo lanciato interrogabile")
        {
            break;
        }
        assert!(
            Instant::now() < lock_deadline,
            "il processo lanciato non ha mantenuto il lock"
        );
        thread::sleep(Duration::from_millis(10));
    }
    execution.join().expect("thread dell'esecutore terminato");
    assert!(!launched_probe
        .is_running(&launched_record)
        .expect("lock rilasciato interrogabile"));

    let record = StepRecord::started(
        "recovery-run",
        "command",
        1,
        1,
        vec![],
        Value::Null,
        vec![],
        10,
    );
    let probe = FileLockProbe::new(directory.join("locks"));
    let lock_path = probe.path_for(&record);
    let executable = env::current_exe().expect("binario della prova");
    let mut child = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "lock_fixture_process",
            "--test-threads=1",
        ])
        .env(LOCK_PATH_ENV, &lock_path)
        .env(READY_PATH_ENV, &ready_path)
        .spawn()
        .expect("processo portatore del lock avviato");
    wait_for_file(&ready_path);
    assert!(probe.is_running(&record).expect("lock vivo interrogabile"));

    let mut executor = ProcessExecutor::new(directory.join("locks"));
    let spec = CommandSpec::new("/bin/sh", &directory, Duration::from_secs(1), 128)
        .with_inspector(NotApplied);
    executor.register("external", spec);
    let mut actions = ActionRegistry::default();
    executor.register_effect_inspectors(&mut actions);
    let mut store = InMemoryRecordStore::from_records(vec![record]);
    let live = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph(),
            run_id: "recovery-run",
            store: &mut store,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &probe,
            clock: &mut FixedClock(20),
        })
        .expect("processo vivo riconciliato");
    assert_eq!(live.still_running, vec!["command"]);

    child.kill().expect("processo portatore del lock ucciso");
    child.wait().expect("processo portatore raccolto");
    assert!(!probe
        .is_running(&store.all()[0])
        .expect("lock morto interrogabile"));
    let dead = InProcessExecutor
        .reconcile(ReconciliationRequest {
            graph: &graph(),
            run_id: "recovery-run",
            store: &mut store,
            actions: &actions,
            shared: &SharedState::new(),
            processes: &probe,
            clock: &mut FixedClock(30),
        })
        .expect("processo morto riconciliato");
    assert_eq!(dead.closed_as_broke, vec!["command"]);
    assert_eq!(store.all()[0].outcome, Some(Outcome::Broke));
    cleanup_directory(&directory);
}

#[test]
fn output_over_the_limit_is_truncated_and_the_record_declares_the_difference() {
    let directory = create_test_directory();
    let script = "i=0; while [ \"$i\" -lt 200 ]; do printf x; i=$((i+1)); done; eval \"printf null >&$SAILOR_OUTPUT_FD\"";
    let store = execute(&directory, script, 32, BTreeMap::new());
    let record = &store.all()[0];
    assert_eq!(record.outcome, Some(Outcome::Went));
    assert_eq!(record.output, Some(Value::Null));
    let said = record
        .said
        .as_deref()
        .expect("metadati dell'uscita registrati");
    assert!(said.starts_with("[launcher-output bytes_seen=200 bytes_kept=32 bytes_discarded=168 configured_limit=32 truncated=true]\n"));
    assert_eq!(
        said.rsplit_once('\n')
            .expect("intestazione separata")
            .1
            .len(),
        32
    );
    cleanup_directory(&directory);
}

#[test]
#[ignore = "avviato soltanto come processo portatore dalla prova padre"]
fn lock_fixture_process() {
    let Ok(lock_path) = env::var(LOCK_PATH_ENV) else {
        return;
    };
    let ready_path = env::var(READY_PATH_ENV).expect("percorso del segnale pronto");
    let _lock = hold_process_lock(Path::new(&lock_path)).expect("lock della vittima preso");
    fs::write(ready_path, b"ready").expect("segnale pronto scritto");
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

fn wait_until_dead(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let result = unsafe { libc::kill(pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn create_test_directory() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("orologio dopo epoch")
        .as_nanos();
    for suffix in 0..100 {
        let path = env::temp_dir().join(format!(
            "launcher-test-{}-{nonce}-{suffix}",
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
            "il processo non ha preso il lock"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn cleanup_directory(directory: &Path) {
    for lock_directory in ["locks", "launched-locks"] {
        let locks = directory.join(lock_directory);
        if let Ok(entries) = fs::read_dir(&locks) {
            for entry in entries {
                fs::remove_file(entry.expect("voce del deposito lock").path())
                    .expect("file lock temporaneo rimosso");
            }
            fs::remove_dir(&locks).expect("deposito lock temporaneo rimosso");
        }
    }
    for name in ["child.pid", "ready"] {
        let path = directory.join(name);
        if path.exists() {
            fs::remove_file(path).expect("file temporaneo rimosso");
        }
    }
    fs::remove_dir(directory).expect("cartella temporanea rimossa");
}
