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
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Nessun altro processo deve nascere mentre una prova tiene un lucchetto
/// ereditabile: chi nasce in quella finestra se lo porta dietro.
static SPAWN_ORDER: Mutex<()> = Mutex::new(());

const LOCK_PATH_ENV: &str = "LAUNCHER_TEST_LOCK_PATH";
const READY_PATH_ENV: &str = "LAUNCHER_TEST_READY_PATH";
const RESULT_PATH_ENV: &str = "LAUNCHER_TEST_RESULT_PATH";

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
    execute_timed(directory, script, limit, environment).0
}

/// Restituisce anche quanto è durato il solo comando: il cronometro parte dopo
/// la coda, altrimenti misura l'attesa delle altre prove invece del tetto.
fn execute_timed(
    directory: &Path,
    script: &str,
    limit: usize,
    environment: BTreeMap<std::ffi::OsString, std::ffi::OsString>,
) -> (InMemoryRecordStore, Duration) {
    let _serial = SPAWN_ORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let started = Instant::now();
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
    (store, started.elapsed())
}

#[test]
fn timeout_kills_a_real_non_terminating_command_and_closes_its_record() {
    let directory = create_test_directory();
    let (store, elapsed) = execute_timed(
        &directory,
        "trap '' TERM; while :; do sleep 1; done",
        128,
        BTreeMap::new(),
    );
    assert!(elapsed < Duration::from_secs(1), "durata reale: {elapsed:?}");
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
    let _serial = SPAWN_ORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
fn contended_lock_finishes_with_timeout_instead_of_blocking_forever() {
    let _serial = SPAWN_ORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = create_test_directory();
    let record = StepRecord::started(
        "run",
        "command",
        1,
        1,
        vec![],
        Value::Null,
        vec!["processes".to_owned()],
        11,
    );
    let probe = FileLockProbe::new(directory.join("locks"));
    let lock_path = probe.path_for(&record);
    let _lock = hold_process_lock(&lock_path).expect("lock concorrente preso");
    let result_path = directory.join("result");
    let executable = env::current_exe().expect("binario della prova");
    let mut child = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "contended_lock_fixture_process",
            "--test-threads=1",
        ])
        .env(LOCK_PATH_ENV, &lock_path)
        .env(RESULT_PATH_ENV, &result_path)
        .spawn()
        .expect("tentativo concorrente avviato");
    let deadline = Instant::now() + Duration::from_secs(1);
    let status = loop {
        if let Some(status) = child.try_wait().expect("tentativo interrogabile") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("tentativo bloccato terminato");
            child.wait().expect("tentativo bloccato raccolto");
            panic!("il lock concorrente non ha prodotto un esito entro 1 secondo");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(&result_path).expect("esito del tentativo scritto"),
        "timeout"
    );
    cleanup_directory(&directory);
}

#[test]
fn file_lock_probe_answers_when_the_lock_file_is_not_writable() {
    let directory = create_test_directory();
    let record = StepRecord::started(
        "read-only-run",
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
    let lock = hold_process_lock(&lock_path).expect("lock preso prima di togliere la scrittura");
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o444))
        .expect("file lock reso non scrivibile");
    assert!(probe.is_running(&record).expect("sonda in sola lettura"));
    drop(lock);
    cleanup_directory(&directory);
}

/// Il lucchetto del padre resta ereditabile fino a `exec`: qualunque processo
/// nato in quella finestra se lo porta dietro. Le altre prove del file lanciano
/// processi propri, quindi in parallelo uno di loro fa fallire questa — e la
/// colpa sembra del codice. Il lucchetto qui serializza le prove che lanciano.
#[test]
fn unrelated_spawn_does_not_inherit_a_lock_held_by_the_parent() {
    let _serial = SPAWN_ORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let directory = create_test_directory();
    let record = StepRecord::started(
        "inheritance-run",
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
    let lock = hold_process_lock(&lock_path).expect("lock del padre preso");
    let mut unrelated = Command::new("/bin/sleep")
        .arg("2")
        .spawn()
        .expect("processo estraneo avviato");
    drop(lock);
    let inherited = probe
        .is_running(&record)
        .expect("lock dopo lo spawn interrogabile");
    unrelated.kill().expect("processo estraneo terminato");
    unrelated.wait().expect("processo estraneo raccolto");
    assert!(!inherited, "il processo estraneo ha ereditato il lock");
    cleanup_directory(&directory);
}

#[test]
fn invalid_output_bytes_report_exactly_the_bytes_shown() {
    let directory = create_test_directory();
    let store = execute(
        &directory,
        "printf '\\377\\377\\377\\377\\377\\377\\377\\377'; eval \"printf null >&$SAILOR_OUTPUT_FD\"",
        8,
        BTreeMap::new(),
    );
    let said = store.all()[0]
        .said
        .as_deref()
        .expect("uscita non valida registrata");
    let (header, shown) = said.split_once('\n').expect("intestazione separata");
    let declared: usize = header
        .split_whitespace()
        .find_map(|field| field.strip_prefix("bytes_kept="))
        .expect("numero dei byte tenuti dichiarato")
        .parse()
        .expect("numero dichiarato valido");
    assert_eq!(declared, shown.len());
    assert_eq!(shown, "��");
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

#[test]
#[ignore = "avviato soltanto come tentativo concorrente dalla prova padre"]
fn contended_lock_fixture_process() {
    let lock_path = env::var(LOCK_PATH_ENV).expect("percorso del lock concorrente");
    let result_path = env::var(RESULT_PATH_ENV).expect("percorso dell'esito");
    let directory = Path::new(&lock_path)
        .parent()
        .and_then(Path::parent)
        .expect("cartella della prova");
    let store = execute(directory, "exit 99", 32, BTreeMap::new());
    let class = store.all()[0]
        .failure_class
        .as_deref()
        .unwrap_or("missing_failure_class");
    fs::write(result_path, class).expect("esito scritto");
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
    for name in ["child.pid", "ready", "result"] {
        let path = directory.join(name);
        if path.exists() {
            fs::remove_file(path).expect("file temporaneo rimosso");
        }
    }
    fs::remove_dir(directory).expect("cartella temporanea rimossa");
}
