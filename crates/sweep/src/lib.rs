mod actions;
mod graph;
mod model;

pub use graph::sweep_graph;
pub use model::*;

use actions::{actions, config_path, read_live};
use flow::{
    Completion, Decision, Execution, ExecutionRequest, Executor, InProcessExecutor, Outcome,
    ProcessProbe, Reconciliation, ReconciliationRequest, RecordStore, SharedState, StepRecord,
    SystemClock,
};
use ledger::Ledger;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn prepare_live_step(
    run_id: &str,
    config: &SweepConfig,
    store: &mut dyn RecordStore,
) -> Result<(), flow::FlowError> {
    if !store.records(run_id)?.is_empty() {
        return Ok(());
    }
    let input = serde_json::to_value(config)
        .map_err(|error| flow::FlowError::InvalidRecord(error.to_string()))?;
    let record = StepRecord::started(
        run_id,
        "read_live_sessions",
        1,
        1,
        vec![],
        input,
        vec!["filesystem".to_owned()],
        now(),
    );
    store.append_started(record)?;
    let live = read_live(&config_path(config));
    let completion = match live {
        Some(live) => Completion {
            outcome: Outcome::Went,
            output: Some(
                serde_json::to_value(live)
                    .map_err(|error| flow::FlowError::InvalidRecord(error.to_string()))?,
            ),
            said: None,
            failure_class: None,
            ended_at: now(),
        },
        None => Completion {
            outcome: Outcome::Waiting,
            output: None,
            said: Some("live session directory is unreadable".to_owned()),
            failure_class: Some("live_sessions_unknown".to_owned()),
            ended_at: now(),
        },
    };
    store.close(run_id, "read_live_sessions", 1, 1, completion)
}

pub fn run(
    run_id: impl Into<String>,
    config: SweepConfig,
    ledger_directory: impl AsRef<Path>,
) -> Result<Execution, Box<dyn std::error::Error>> {
    let run_id = run_id.into();
    let graph = sweep_graph();
    let actions = actions();
    let mut ledger = Ledger::open(ledger_directory)?;
    prepare_live_step(&run_id, &config, &mut ledger)?;
    let request = ExecutionRequest {
        run_id,
        root_inputs: [("scan_markers".to_owned(), serde_json::to_value(config)?)]
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
    use flow::{Action, ActionRegistry, EffectStatus};
    use std::env;
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

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
        assert_eq!(live.failure_class.as_deref(), Some("live_sessions_unknown"));
    }

    #[test]
    fn ledger_trace_names_looked_orphan_and_removed_markers() {
        let root = directory("trace");
        let state = root.join("state");
        let ledger = root.join("ledger");
        fs::create_dir_all(state.join("sessioni-vive")).unwrap();
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
        assert_eq!(output.removed, ["one"]);
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
