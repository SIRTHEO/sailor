mod actions;
mod graph;
mod model;

pub use graph::sweep_graph;
pub use model::*;

use actions::actions;
use flow::{
    Decision, Execution, ExecutionRequest, Executor, InProcessExecutor, ProcessProbe,
    Reconciliation, ReconciliationRequest, SharedState, StepRecord, SystemClock,
};
use ledger::Ledger;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

const LOCK_STALE_SECS: u64 = 10 * 60;

struct SweepLock {
    path: std::path::PathBuf,
}

enum TakeLock {
    Taken(SweepLock),
    Locked,
    Io(std::io::Error),
}

impl SweepLock {
    fn take(state: &Path) -> TakeLock {
        let path = state.join("marker-sweep.lock");
        match Self::create(&path) {
            TakeLock::Taken(lock) => return TakeLock::Taken(lock),
            TakeLock::Io(error) => return TakeLock::Io(error),
            TakeLock::Locked => {}
        }
        let stale = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|elapsed| elapsed.as_secs() >= LOCK_STALE_SECS);
        if stale {
            let _ = fs::remove_file(&path);
            Self::create(&path)
        } else if path.exists() {
            TakeLock::Locked
        } else {
            Self::create(&path)
        }
    }

    fn create(path: &Path) -> TakeLock {
        use std::os::unix::fs::OpenOptionsExt;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", std::process::id());
                TakeLock::Taken(Self {
                    path: path.to_owned(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => TakeLock::Locked,
            Err(error) => TakeLock::Io(error),
        }
    }
}

impl Drop for SweepLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn run(
    run_id: impl Into<String>,
    config: SweepConfig,
    ledger_directory: impl AsRef<Path>,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let run_id = run_id.into();
    let _lock = match SweepLock::take(Path::new(&config.state_dir)) {
        TakeLock::Taken(lock) => lock,
        TakeLock::Locked => {
            return Ok(Execution {
                decisions: vec![Decision::Waiting(vec!["sweep_lock".to_owned()])],
                shared: SharedState::new(),
            });
        }
        TakeLock::Io(error) => return Err(Box::new(error)),
    };
    let graph = sweep_graph();
    let actions = actions();
    let mut ledger = Ledger::open(ledger_directory)?;
    let input = serde_json::to_value(config)?;
    let request = ExecutionRequest {
        run_id,
        root_inputs: [
            ("scan_markers".to_owned(), input.clone()),
            ("read_live_sessions".to_owned(), input),
        ]
        .into_iter()
        .collect(),
        gates: vec!["filesystem".to_owned()],
        shared: BTreeMap::new(),
    };
    Ok(InProcessExecutor.execute(&graph, request, &mut ledger, &actions, &mut SystemClock)?)
}

struct NoProcess;

impl ProcessProbe for NoProcess {
    fn is_running(&self, _record: &StepRecord) -> Result<bool, flow::FlowError> {
        Ok(false)
    }
}

pub fn reconcile(
    run_id: &str,
    ledger_directory: impl AsRef<Path>,
) -> Result<Reconciliation, Box<dyn std::error::Error>> {
    let graph = sweep_graph();
    let actions = actions();
    let mut ledger = Ledger::open(ledger_directory)?;
    Ok(InProcessExecutor.reconcile(ReconciliationRequest {
        graph: &graph,
        run_id,
        store: &mut ledger,
        actions: &actions,
        shared: &SharedState::new(),
        processes: &NoProcess,
        clock: &mut SystemClock,
    })?)
}

pub fn trace(
    run_id: &str,
    ledger_directory: impl AsRef<Path>,
) -> Result<Vec<StepRecord>, ledger::LedgerError> {
    Ledger::open(ledger_directory)?.steps(run_id)
}

pub fn decision(
    run_id: &str,
    ledger_directory: impl AsRef<Path>,
) -> Result<Decision, Box<dyn std::error::Error>> {
    let ledger = Ledger::open(ledger_directory)?;
    Ok(InProcessExecutor.decision(&sweep_graph(), run_id, &ledger)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::RemoveAction;
    use flow::{Action, ActionOutcome, ActionRegistry, EffectStatus, Outcome, RecordStore};
    use std::env;
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const FIXTURE_LEDGER: &str = "SWEEP_TEST_LEDGER";
    const FIXTURE_STATE: &str = "SWEEP_TEST_STATE";
    const FIXTURE_SIGNAL: &str = "SWEEP_TEST_PAUSE_AFTER_FIRST";

    fn directory(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("sweep-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    fn old_marker(state: &Path, name: &str) {
        let path = state.join(name);
        fs::write(&path, b"marker").unwrap();
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(UNKNOWN_GRACE_SECS + 2))
            .unwrap();
    }

    fn gone_session(state: &Path, short: &str, full: &str) {
        fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        fs::write(
            state.join("sessioni-vive").join(format!("{short}.json")),
            format!(
                r#"{{"session_id":"{full}","session_pid":4000000000,"updated_at":0}}"#
            ),
        )
        .unwrap();
    }

    fn legacy_marker(state: &Path, marker_path: &str, owner: &str) -> String {
        let hex = guards::successor::armed_fingerprint(marker_path, owner);
        let name = format!("successore-armato-{hex}");
        fs::write(
            state.join(&name),
            format!("2026-08-18T00:00:00\n{marker_path}\n"),
        )
        .unwrap();
        name
    }

    #[test]
    fn missing_live_directory_is_waiting_and_removes_nothing() {
        let root = directory("unknown");
        let state = root.join("state");
        let ledger = root.join("ledger");
        fs::create_dir(&state).unwrap();
        let hex = guards::successor::armed_fingerprint("/x/consegna.md", "live-session");
        let name = format!("successore-armato-{hex}");
        fs::write(state.join(&name), "2026-08-18T00:00:00\n/x/consegna.md\n").unwrap();

        let execution = run(
            "unknown-run",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            &ledger,
        )
        .unwrap();

        assert_eq!(
            execution.decisions.last(),
            Some(&Decision::Waiting(vec!["read_live_sessions".to_owned()]))
        );
        assert!(state.join(name).exists());
        let records = trace("unknown-run", &ledger).unwrap();
        let live = records
            .iter()
            .find(|record| record.step_id == "read_live_sessions")
            .unwrap();
        assert_eq!(live.outcome, Some(Outcome::Waiting));
        assert_eq!(live.failure_class, None);
    }

    #[test]
    fn ledger_trace_names_looked_orphan_and_removed_markers() {
        let root = directory("trace");
        let state = root.join("state");
        let ledger = root.join("ledger");
        gone_session(&state, "deadbeef", "deadbeef-dead-dead-dead-deadbeef0000");
        old_marker(&state, "consegna-misura-deadbeef");

        run(
            "trace-run",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            &ledger,
        )
        .unwrap();

        let records = trace("trace-run", &ledger).unwrap();
        let output: RemovalTrace = serde_json::from_value(
            records
                .iter()
                .find(|record| record.step_id == "remove_markers")
                .unwrap()
                .output
                .clone()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(output.looked, ["consegna-misura-deadbeef"]);
        assert_eq!(output.orphan, ["consegna-misura-deadbeef"]);
        assert_eq!(output.removed, ["consegna-misura-deadbeef"]);
    }

    #[test]
    fn unreadable_live_entry_stops_every_removal() {
        let root = directory("partial-live");
        let state = root.join("state");
        let ledger = root.join("ledger");
        fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        fs::write(state.join("sessioni-vive/broken.json"), b"not json").unwrap();
        old_marker(&state, "consegna-misura-deadbeef");
        let legacy = legacy_marker(&state, "/x/live.md", "live-session");

        let execution = run(
            "partial-run",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            &ledger,
        )
        .unwrap();

        assert_eq!(
            execution.decisions.last(),
            Some(&Decision::Waiting(vec!["read_live_sessions".to_owned()]))
        );
        assert!(state.join("consegna-misura-deadbeef").exists());
        assert!(state.join(legacy).exists());
    }

    #[test]
    fn empty_live_directory_spares_legacy_markers() {
        let root = directory("empty-live");
        let state = root.join("state");
        let ledger = root.join("ledger");
        fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        let legacy = legacy_marker(&state, "/x/live.md", "live-session");

        let execution = run(
            "empty-run",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            &ledger,
        )
        .unwrap();

        assert_eq!(
            execution.decisions.last(),
            Some(&Decision::Waiting(vec!["read_live_sessions".to_owned()]))
        );
        assert!(state.join(legacy).exists());
    }

    #[test]
    #[ignore = "avviato soltanto dalla prova padre"]
    fn crash_fixture_process() {
        let Ok(ledger_path) = env::var(FIXTURE_LEDGER) else {
            return;
        };
        let state = env::var(FIXTURE_STATE).unwrap();
        let plan = RemovalPlan {
            state_dir: state,
            deleting: true,
            looked: vec!["one".to_owned(), "two".to_owned()],
            orphan: vec!["one".to_owned(), "two".to_owned()],
            targets: ["one", "two"]
                .into_iter()
                .map(|name| RemovalTarget {
                    name: name.to_owned(),
                    kind: "standard".to_owned(),
                    session: "deadbeef".to_owned(),
                    liveness: Liveness::Gone,
                })
                .collect(),
        };
        let input = serde_json::to_value(&plan).unwrap();
        let mut store = Ledger::open(ledger_path).unwrap();
        store
            .append_started(StepRecord::started(
                "crash-run",
                "remove_markers",
                1,
                1,
                vec!["plan_removals".to_owned()],
                input.clone(),
                vec!["filesystem".to_owned()],
                1,
            ))
            .unwrap();
        RemoveAction
            .execute(&input, &mut SharedState::new())
            .unwrap();
    }

    #[test]
    #[ignore = "avviato soltanto dalla prova padre"]
    fn locked_sweep_fixture_process() {
        let Ok(ledger) = env::var(FIXTURE_LEDGER) else {
            return;
        };
        let state = std::path::PathBuf::from(env::var(FIXTURE_STATE).unwrap());
        run(
            "locked-child",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            ledger,
        )
        .unwrap();
    }

    #[test]
    fn two_sweeps_do_not_remove_in_contention() {
        let root = directory("lock");
        let state = root.join("state");
        let first_ledger = root.join("first-ledger");
        let second_ledger = root.join("second-ledger");
        let signal = root.join("first");
        fs::create_dir(&state).unwrap();
        gone_session(&state, "deadbeef", "deadbeef-dead-dead-dead-deadbeef0000");
        old_marker(&state, "consegna-misura-deadbeef");
        old_marker(&state, "consegna-stop-deadbeef");
        let mut child = Command::new(env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "tests::locked_sweep_fixture_process",
                "--test-threads=1",
            ])
            .env(FIXTURE_LEDGER, &first_ledger)
            .env(FIXTURE_STATE, &state)
            .env(FIXTURE_SIGNAL, &signal)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !signal.exists() {
            assert!(Instant::now() < deadline, "la prima passata non si è fermata");
            thread::sleep(Duration::from_millis(10));
        }

        let second = run(
            "locked-parent",
            SweepConfig {
                state_dir: state.to_string_lossy().into_owned(),
                deleting: true,
            },
            &second_ledger,
        )
        .unwrap();
        assert_eq!(
            second.decisions,
            [Decision::Waiting(vec!["sweep_lock".to_owned()])]
        );
        assert!(state.join("consegna-stop-deadbeef").exists());
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
    }

    #[test]
    fn restarted_session_keeps_its_rewritten_standard_marker() {
        let root = directory("restart");
        let state = root.join("state");
        fs::create_dir(&state).unwrap();
        old_marker(&state, "consegna-misura-deadbeef");
        let plan = RemovalPlan {
            state_dir: state.to_string_lossy().into_owned(),
            deleting: true,
            looked: vec!["consegna-misura-deadbeef".to_owned()],
            orphan: vec!["consegna-misura-deadbeef".to_owned()],
            targets: vec![RemovalTarget {
                name: "consegna-misura-deadbeef".to_owned(),
                kind: "standard".to_owned(),
                session: "deadbeef".to_owned(),
                liveness: Liveness::Gone,
            }],
        };

        fs::write(state.join("consegna-misura-deadbeef"), b"rewritten").unwrap();
        fs::create_dir(state.join("sessioni-vive")).unwrap();
        fs::write(state.join("sessioni-vive/deadbeef.json"), b"starting").unwrap();
        let output = RemoveAction
            .execute(&serde_json::to_value(plan).unwrap(), &mut SharedState::new())
            .unwrap();
        let ActionOutcome::Went(value) = output else {
            panic!("la rimozione deve produrre una traccia")
        };
        let trace: RemovalTrace = serde_json::from_value(value).unwrap();

        assert_eq!(trace.spared, ["consegna-misura-deadbeef"]);
        assert!(state.join("consegna-misura-deadbeef").exists());
    }

    #[test]
    fn vanished_file_is_not_claimed_during_recovery() {
        let root = directory("vanished");
        let state = root.join("state");
        fs::create_dir(&state).unwrap();
        let plan = RemovalPlan {
            state_dir: state.to_string_lossy().into_owned(),
            deleting: true,
            looked: vec!["gone".to_owned()],
            orphan: vec!["gone".to_owned()],
            targets: vec![RemovalTarget {
                name: "gone".to_owned(),
                kind: "standard".to_owned(),
                session: "deadbeef".to_owned(),
                liveness: Liveness::Gone,
            }],
        };
        let record = StepRecord::started(
            "vanished-run",
            "remove_markers",
            1,
            1,
            vec!["plan_removals".to_owned()],
            serde_json::to_value(plan).unwrap(),
            vec!["filesystem".to_owned()],
            1,
        );
        let EffectStatus::Applied(value) = RemoveAction
            .inspect_effect(&record, &SharedState::new())
            .unwrap()
        else {
            panic!("la ripresa deve chiudere la rimozione")
        };
        let trace: RemovalTrace = serde_json::from_value(value).unwrap();

        assert!(trace.removed.is_empty());
        assert_eq!(trace.vanished, ["gone"]);
    }

    #[test]
    fn killed_mid_removal_is_closed_from_disk_without_redeleting() {
        let root = directory("crash");
        let state = root.join("state");
        let ledger_path = root.join("ledger");
        let signal = root.join("first");
        fs::create_dir(&state).unwrap();
        old_marker(&state, "one");
        old_marker(&state, "two");
        let executable = env::current_exe().unwrap();
        let mut child = Command::new(executable)
            .args([
                "--ignored",
                "--exact",
                "tests::crash_fixture_process",
                "--test-threads=1",
            ])
            .env(FIXTURE_LEDGER, &ledger_path)
            .env(FIXTURE_STATE, &state)
            .env(FIXTURE_SIGNAL, &signal)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !signal.exists() {
            assert!(
                Instant::now() < deadline,
                "il figlio non ha rimosso il primo marcatore"
            );
            thread::sleep(Duration::from_millis(10));
        }
        child.kill().unwrap();
        assert!(!child.wait().unwrap().success());
        assert!(!state.join("one").exists());
        assert!(state.join("two").exists());

        let report = reconcile("crash-run", &ledger_path).unwrap();
        assert_eq!(report.closed_as_went, ["remove_markers"]);
        assert!(
            state.join("two").exists(),
            "la riconciliazione ha rilanciato la cancellazione"
        );
        let records = trace("crash-run", &ledger_path).unwrap();
        let output: RemovalTrace =
            serde_json::from_value(records[0].output.clone().unwrap()).unwrap();
        assert!(output.removed.is_empty());
        assert_eq!(output.vanished, ["one"]);
        assert_eq!(output.spared, ["two"]);
        assert!(output.recovered);

        let mut registry = ActionRegistry::default();
        registry.register("remove_markers", RemoveAction);
        let status = registry
            .get("remove_markers")
            .unwrap()
            .inspect_effect(&records[0], &SharedState::new())
            .unwrap();
        assert!(matches!(status, EffectStatus::Applied(_)));
    }
}
