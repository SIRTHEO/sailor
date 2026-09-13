//! `take-the-next-work`, run for real: `CHEAP_WORKER` resolves through
//! `FakeCheapWorker`, no call spent. A shipped flow, read off the binary at
//! test time, the same as a fresh install would find it.

use actions::{AskRecipe, PromptVia, ToolResolver};
use flow::{
    ActionRegistry, Decision, Execution, ExecutionRequest, Executor, Graph, InMemoryRecordStore,
    InProcessExecutor, Outcome, RunStops, SharedState, StopReason, SystemClock,
};
use ledger::{Ledger, StoreRecord};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

/// The mandate's stdin opens with `Mandate <digest>: …`; a worker of good
/// faith echoes that digest back as `ack <digest>` before doing anything
/// else. Shared by both fakes below, so the acknowledgment line is read the
/// same way whether the worker then does the work or leaves the tree clean.
const ACK_BACK: &str = "input=$(cat); \
    digest=$(printf '%s' \"$input\" | head -n 1 | sed -E 's/^Mandate ([^:]+):.*/\\1/')";

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
                format!(
                    "{ACK_BACK}; if [ -n \"$input\" ]; then touch TASK_OK; fi; \
                     printf 'ack %s\\nattempted' \"$digest\""
                ),
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

/// A worker that touches nothing: the tree `execute` cut comes back exactly as
/// clean as it was cut, which is the case fault 182 broke.
struct SilentCheapWorker;

impl ToolResolver for SilentCheapWorker {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }

    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        Some(AskRecipe {
            args: vec![
                "-c".to_owned(),
                format!("{ACK_BACK}; printf 'ack %s\\ndid nothing' \"$digest\""),
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

fn full_graph() -> Graph {
    let flow = flow::system::builtin_registry()
        .remove("take-the-next-work")
        .expect("the flow is shipped")
        .expect("the shipped flow loads");
    Graph::new(flow.graph.steps().to_vec()).expect("the shipped graph stays valid")
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
    for project in ["alpha", "beta", "gamma"] {
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

/// Whether a step of this id ever closed `Went`: the only outcome that means
/// its command actually ran and answered, as opposed to being skipped by its
/// own `when` or never reached at all.
fn step_went(store: &InMemoryRecordStore, step_id: &str) -> bool {
    store
        .all()
        .iter()
        .any(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went))
}

/// A worker that never opens its answer with the acknowledgment line: it
/// answers plainly, the way a mandate misread would.
struct UnacknowledgingWorker;

impl ToolResolver for UnacknowledgingWorker {
    fn resolve(&self, _id: &str) -> Result<String, String> {
        Ok("sh".to_owned())
    }

    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        Some(AskRecipe {
            args: vec![
                "-c".to_owned(),
                "cat >/dev/null; printf 'done'".to_owned(),
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

/// Fault 182: `execute` used to close its own tree the moment it returned,
/// success included. A worker that leaves no artefact behind left the tree
/// clean, so that close **succeeded** right there — and `acceptance`, reading
/// the same tree afterwards, found no workdir left to run `./check.sh` in.
/// The task's own check never ran; it was recorded as failing anyway.
#[test]
fn a_worker_that_leaves_the_tree_clean_still_gets_it_read_by_acceptance() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("clean-tree");
    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({"tools": ["claude-code"]}),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("the role is written");
    ledger
        .put_record(&StoreRecord {
            collection: "work-queue".to_owned(),
            key: "gamma-top".to_owned(),
            value: json!({
                "project": "gamma",
                "title": "the gamma task",
                "priority": 100,
                "state": "queued",
                "attempts": 0,
                "acceptance": "./check.sh",
                "workspace": fixtures.join("gamma").to_string_lossy(),
            }),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("a queue record is written");

    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(SilentCheapWorker)
            .recording_to(Some(ledger.clone())),
    );
    actions::store::register_store(&mut registry, Some(ledger.clone()));
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new(),
    );

    let graph = full_graph();
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        json!(fixtures.join("gamma").to_string_lossy()),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: "clean-tree-1".to_owned(),
        root_inputs: [("trigger".to_owned(), trigger_input("gamma"))]
            .into_iter()
            .collect(),
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &registry, &SystemClock)
        .expect("the run executes without breaking the engine itself");

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "{:?}",
        execution.decisions
    );

    let gamma = ledger
        .read_record("work-queue", "gamma-top")
        .expect("work-queue reads")
        .expect("gamma-top is still there");
    assert_eq!(
        gamma.value["state"], "done",
        "the check ran in a tree that was still there, not one closed out from \
         under it: {:?}",
        gamma.value
    );
}

#[test]
fn a_step_with_a_tree_of_its_own_and_a_repo_cuts_from_that_repo() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("repo-override");
    
    
    let wrong_repo = fixtures.join("beta");
    
    
    let right_repo = fixtures.join("alpha");

    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({"tools": ["claude-code"]}),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("the role is written");

    ledger
        .put_record(&StoreRecord {
            collection: "work-queue".to_owned(),
            key: "alpha-top".to_owned(),
            value: json!({
                "project": "alpha",
                "title": "the alpha task",
                "priority": 100,
                "state": "queued",
                "attempts": 0,
                "acceptance": "./check.sh",
                "workspace": right_repo.to_string_lossy(),
            }),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("a queue record is written");

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

    let graph = full_graph();
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    
    
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        json!(wrong_repo.to_string_lossy()),
    );
    
    let request = ExecutionRequest {
        holder: None,
        run_id: "repo-override-1".to_owned(),
        root_inputs: [("trigger".to_owned(), trigger_input("alpha"))]
            .into_iter()
            .collect(),
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &registry, &SystemClock)
        .expect("the run executes");

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "execution completed: {:?}", execution.decisions
    );

    
    let trees = ledger.records_in("open-worktrees").expect("reads open-worktrees");
    assert_eq!(trees.len(), 1, "one tree cut");
    
    let path = trees[0].value["path"].as_str().expect("path is a string");
    let repo_in_record = trees[0].value["repo"].as_str().expect("repo is a string");
    
    assert_eq!(
        repo_in_record,
        right_repo.to_string_lossy(),
        "the record names the right repository"
    );
    assert!(
        path.starts_with(right_repo.to_string_lossy().as_ref()),
        "the tree on disk ({}) was cut from the right repository ({})",
        path,
        right_repo.display()
    );
}

/// The digest computed before `execute` and the acknowledgment read from the
/// worker's own answer are the same value, both landing in the task's record
/// once the run completes.
#[test]
fn a_worker_that_acknowledges_the_mandate_finishes_and_its_record_carries_the_digest() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("ack-ok");
    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({"tools": ["claude-code"]}),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("the role is written");
    ledger
        .put_record(&StoreRecord {
            collection: "work-queue".to_owned(),
            key: "gamma-ack".to_owned(),
            value: json!({
                "project": "gamma",
                "title": "the gamma ack task",
                "priority": 100,
                "state": "queued",
                "attempts": 0,
                "acceptance": "./check.sh",
                "workspace": fixtures.join("gamma").to_string_lossy(),
            }),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("a queue record is written");

    let graph = full_graph();
    let (execution, _store) = run_once(&graph, &ledger, "ack-ok-1", "gamma", &fixtures);

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "{:?}",
        execution.decisions
    );

    let task = ledger
        .read_record("work-queue", "gamma-ack")
        .expect("work-queue reads")
        .expect("gamma-ack is still there");
    assert_eq!(task.value["state"], "done", "{:?}", task.value);
    let digest = task.value["mandate_digest"]
        .as_str()
        .expect("mandate_digest is a string");
    assert!(!digest.is_empty(), "{:?}", task.value);
    assert_eq!(
        task.value["acknowledged"], task.value["mandate_digest"],
        "the worker's acknowledgment matches the digest it was handed: {:?}",
        task.value
    );
}

/// The counterexample fault 182 already proved for a clean tree, proved again
/// for an unacknowledged mandate: the acceptance command must never run once
/// the worker's answer does not open with `ack <digest>`, and the task is
/// parked instead.
#[test]
fn a_worker_that_never_acknowledges_the_mandate_is_parked_before_acceptance_ever_runs() {
    let _fixtures_lock = FIXTURES_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let fixtures = make_fixtures();
    let ledger = fresh_ledger("ack-missing");
    ledger
        .put_record(&StoreRecord {
            collection: "roles".to_owned(),
            key: "CHEAP_WORKER".to_owned(),
            value: json!({"tools": ["claude-code"]}),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("the role is written");
    ledger
        .put_record(&StoreRecord {
            collection: "work-queue".to_owned(),
            key: "gamma-no-ack".to_owned(),
            value: json!({
                "project": "gamma",
                "title": "the gamma task nobody acknowledges",
                "priority": 100,
                "state": "queued",
                "attempts": 0,
                "acceptance": "./check.sh",
                "workspace": fixtures.join("gamma").to_string_lossy(),
            }),
            written_by: "test".to_owned(),
            written_at: 0,
        })
        .expect("a queue record is written");

    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(UnacknowledgingWorker)
            .recording_to(Some(ledger.clone())),
    );
    actions::store::register_store(&mut registry, Some(ledger.clone()));
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new(),
    );

    let graph = full_graph();
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        json!(fixtures.join("gamma").to_string_lossy()),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: "ack-missing-1".to_owned(),
        root_inputs: [("trigger".to_owned(), trigger_input("gamma"))]
            .into_iter()
            .collect(),
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&graph, request, &store, &registry, &SystemClock)
        .expect("the run executes without breaking the engine itself");

    assert!(
        matches!(execution.decisions.last(), Some(Decision::Waiting(_))),
        "a parked task waits on its handoff to a person: {:?}",
        execution.decisions
    );
    assert!(
        !step_went(&store, "acceptance"),
        "the acceptance command must never run once the mandate goes unacknowledged"
    );

    let task = ledger
        .read_record("work-queue", "gamma-no-ack")
        .expect("work-queue reads")
        .expect("gamma-no-ack is still there");
    assert_eq!(task.value["state"], "parked", "{:?}", task.value);
    assert_eq!(
        task.value["reason"], "the worker did not acknowledge the mandate",
        "{:?}",
        task.value
    );
    assert!(
        task.value.get("acknowledged").is_none(),
        "an unacknowledged mandate never gets that field written: {:?}",
        task.value
    );
}
