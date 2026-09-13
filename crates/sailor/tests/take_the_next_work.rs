//! `take-the-next-work`, run for real: `CHEAP_WORKER` resolves through
//! `FakeCheapWorker`, no call spent. A home flow, read off `flows/` at test time.

use actions::{AskRecipe, PromptVia, ToolResolver};
use flow::{
    ActionRegistry, Decision, Execution, ExecutionRequest, Executor, FlowFile, Graph,
    InMemoryRecordStore, InProcessExecutor, RunStops, SharedState, StopReason, SystemClock,
};
use ledger::{Ledger, StoreRecord};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

/// Fakes `ask_recipe` instead of a step-written `args`: `record_the_call`
/// never bills a hand-written `args` as a model call, the same as `git`.
struct FakeCheapWorker;

impl ToolResolver for FakeCheapWorker {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }

    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        Some(AskRecipe {
            args: vec![
                "-c".to_owned(),
                "input=$(cat); if [ -n \"$input\" ]; then touch TASK_OK; fi; printf 'attempted'"
                    .to_owned(),
                "worker".to_owned(),
            ],
            prompt: PromptVia::Stdin,
            args_before_prompt: Vec::new(),
            unusable_when: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            waits_for_a_person_when: Vec::new(),
            silent_without_prompt: false,
            refuses_without_prompt: Vec::new(),
            usage: None,
        })
    }
}

fn flow_path() -> PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../flows/take-the-next-work.flow.json"))
        .to_path_buf()
}

fn flow_file() -> FlowFile {
    let text = std::fs::read_to_string(flow_path())
        .unwrap_or_else(|error| panic!("reading {}: {error}", flow_path().display()));
    serde_json::from_str(&text).expect("the flow loads as a FlowFile")
}

fn full_graph() -> Graph {
    Graph::new(flow_file().graph.steps().to_vec()).expect("the shipped graph stays valid")
}

/// Guards `make-fixtures.sh`'s `rm -rf`: `cargo test` runs a binary's tests
/// concurrently, so one test's rebuild deleted another's fixtures mid-run.
static FIXTURES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn make_fixtures() -> PathBuf {
    let script = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../flows/tests/make-fixtures.sh"
    ));
    let output = std::process::Command::new("sh")
        .arg(script)
        .output()
        .expect("make-fixtures.sh runs");
    assert!(
        output.status.success(),
        "make-fixtures.sh failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fixtures = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/fixtures")).to_path_buf();
    // A just-`chmod +x`-ed script pays a one-time first-exec cost here; paying
    // it now keeps the flow's own acceptance timeout a measure of the check.
    for project in ["alpha", "beta"] {
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg("./check.sh")
            .current_dir(fixtures.join(project))
            .output();
    }
    fixtures
}

fn fresh_ledger(tag: &str) -> Ledger {
    let dir = std::env::temp_dir().join(format!(
        "queue-flow-{tag}-{}-{}",
        std::process::id(),
        tag.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    Ledger::open(&dir).unwrap_or_else(|error| panic!("opening a test ledger at {}: {error}", dir.display()))
}

/// The role and the one queued task per project this test needs: `alpha`'s
/// task the cheap worker can finish, `beta`'s the one it never can.
fn seed(ledger: &Ledger, fixtures: &Path) {
    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({"tools": ["claude-code"]}),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("the role is written");

    let task = |key: &str, project: &str, priority: i64| StoreRecord {
        collection: "work-queue".to_owned(),
        key: key.to_owned(),
        value: json!({
            "project": project,
            "title": format!("the {project} task"),
            "priority": priority,
            "state": "queued",
            "attempts": 0,
            "acceptance": "./check.sh",
            "workspace": fixtures.join(project).to_string_lossy(),
        }),
        written_by: "test".to_owned(),
        written_at: 0,
    };
    for record in [task("alpha-top", "alpha", 100), task("beta-top", "beta", 90)] {
        ledger.put_record(&record).expect("a queue record is written");
    }
}

fn registry_over(ledger: &Ledger) -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(FakeCheapWorker)
            .recording_to(Some(ledger.clone())),
    );
    actions::store::register_store(&mut registry, Some(ledger.clone()));
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new(),
    );
    registry
}

fn trigger_input(project: &str) -> Value {
    json!({"source": "manual", "text": project})
}

fn run_once(
    graph: &Graph,
    ledger: &Ledger,
    run_id: &str,
    project: &str,
    fixtures: &Path,
) -> (Execution, InMemoryRecordStore) {
    let registry = registry_over(ledger);
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        json!(fixtures.join(project).to_string_lossy()),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: run_id.to_owned(),
        root_inputs: [("trigger".to_owned(), trigger_input(project))]
            .into_iter()
            .collect(),
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(graph, request, &store, &registry, &SystemClock)
        .expect("the run executes without breaking the engine itself");
    (execution, store)
}

fn ran_execute(store: &InMemoryRecordStore) -> bool {
    store.all().iter().any(|record| record.step_id == "execute")
}

/// The fault the claim step closes, measured directly: a real-thread version
/// was flaky, since nothing kept one runner from finishing before the other started.
#[test]
fn without_a_claim_two_runners_can_both_declare_the_same_task_done() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("red");
    seed(&ledger, &fixtures);
    let registry = registry_over(&ledger);
    let shared = SharedState::new();

    let select = registry
        .get(actions::store::STORE_SELECT_ACTION)
        .expect("store_select is registered");
    let select_input = json!({
        "collection": "work-queue",
        "where": {"state": "queued", "project": "alpha"},
        "order_by": {"field": "priority", "direction": "desc"},
        "limit": 1
    });

    let first = select.execute(&select_input, &shared).expect("runner one selects");
    let second = select
        .execute(&select_input, &shared)
        .expect("runner two selects, with nothing written in between");
    let flow::ActionOutcome::Went(first) = first else {
        panic!("a selection always answers");
    };
    let flow::ActionOutcome::Went(second) = second else {
        panic!("a selection always answers");
    };
    assert_eq!(first["records"][0]["key"], "alpha-top", "{first}");
    assert_eq!(
        first, second,
        "two runners racing the same queue see the identical unclaimed task: {first} vs {second}"
    );

    let write = registry
        .get(actions::store::STORE_WRITE_ACTION)
        .expect("store_write is registered");
    for runner in ["runner-one", "runner-two"] {
        let outcome = write.execute(
            &json!({
                "collection": "work-queue",
                "key": "alpha-top",
                "value": {"project": "alpha", "state": "done", "attempts": 1},
                "written_by": runner
            }),
            &shared,
        );
        assert!(
            outcome.is_ok(),
            "without a claim, nothing stops {runner} from declaring the task done too: {outcome:?}"
        );
    }
}

/// The proof the claim step closes it: four runners race the same store, two
/// per project, and each task is taken by exactly one.
#[test]
fn two_runners_racing_the_same_store_take_each_task_at_most_once() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("green");
    seed(&ledger, &fixtures);

    let graph = Arc::new(full_graph());
    let barrier = Arc::new(Barrier::new(4));
    let runs = [
        ("green-alpha-1", "alpha"),
        ("green-alpha-2", "alpha"),
        ("green-beta-1", "beta"),
        ("green-beta-2", "beta"),
    ];
    let handles: Vec<_> = runs
        .into_iter()
        .map(|(run_id, project)| {
            let graph = Arc::clone(&graph);
            let ledger = ledger.clone();
            let barrier = Arc::clone(&barrier);
            let fixtures = fixtures.clone();
            std::thread::spawn(move || {
                barrier.wait();
                run_once(&graph, &ledger, run_id, project, &fixtures)
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|handle| handle.join().expect("the thread joins")).collect();

    for (execution, _) in &results {
        assert!(
            matches!(
                execution.decisions.last(),
                Some(Decision::Complete)
                    | Some(Decision::RequirementUnmet { .. })
                    | Some(Decision::Halted { reason: StopReason::Promise, .. })
                    | Some(Decision::Waiting(_))
            ),
            "a lost claim halts on its own `stops_when` (`Promise`); a task the \
             acceptance never passed closes with the requirement unmet and, once \
             parked, waits on its handoff to a person; the winner that finishes \
             closes complete — but nothing here should break: {:?}",
            execution.decisions
        );
    }

    let claims = ledger.records_in("work-queue-claims").expect("the claims collection reads");
    assert_eq!(
        claims.len(),
        2,
        "one claim per task actually taken, not per runner that asked: {claims:?}"
    );

    let executed = results.iter().filter(|(_, store)| ran_execute(store)).count();
    assert_eq!(
        executed, 2,
        "exactly one runner per project reaches execute; the other's claim is lost"
    );

    let alpha = ledger
        .read_record("work-queue", "alpha-top")
        .expect("work-queue reads")
        .expect("alpha-top is still there");
    assert_eq!(alpha.value["state"], "done", "{:?}", alpha.value);

    let beta = ledger
        .read_record("work-queue", "beta-top")
        .expect("work-queue reads")
        .expect("beta-top is still there");
    assert_eq!(beta.value["state"], "parked", "{:?}", beta.value);
    assert!(
        !beta.value["reason"].as_str().unwrap_or_default().is_empty(),
        "a parked task carries why: {:?}",
        beta.value
    );

    let calls = ledger
        .browse("SELECT role, role_resolved_to FROM model_calls", 10)
        .expect("model_calls reads");
    let role_at = calls.columns.iter().position(|column| column == "role").expect("a role column");
    let resolved_at = calls
        .columns
        .iter()
        .position(|column| column == "role_resolved_to")
        .expect("a role_resolved_to column");
    assert_eq!(calls.rows.len(), 2, "one recorded call per task actually run: {:?}", calls.rows);
    for row in &calls.rows {
        assert_eq!(row[role_at], json!("CHEAP_WORKER"), "{row:?}");
        assert_ne!(row[resolved_at], Value::Null, "the tool the role resolved to is named: {row:?}");
    }
}
