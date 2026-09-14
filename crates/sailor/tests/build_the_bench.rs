//! The shipped `build-the-bench` and its child `validate-a-bench-task`: the
//! two name only actions the product registers, and one run of the parent
//! over a repository of the test's own writes one validated task, hashes the
//! set and records it in the store. No engine runs and nothing of this
//! machine is read: the repository, the home and the store are all scratch.

use flow::system::{load_all, FlowSource};
use flow::{
    ActionRegistry, Clock, ExecutionRequest, Executor, FlowError, FlowFile, InMemoryRecordStore,
    InProcessExecutor, Outcome, SharedState,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

const PARENT: &str = "build-the-bench";
const CHILD: &str = "validate-a-bench-task";

fn shipped(id: &str) -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == id)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .unwrap_or_else(|| panic!("«{id}» is shipped"))
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-build-the-bench-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to work in");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn registry(scratch: &Scratch) -> (ActionRegistry, ledger::Ledger) {
    let ledger = ledger::Ledger::open(scratch.0.join("ledger")).expect("a store of our own");
    let registry = registry::registry_in(registry::House::under(&scratch.0), Some(ledger.clone()), None);
    (registry, ledger)
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

/// Both flows name only actions the product's registry carries.
#[test]
fn the_two_shipped_flows_name_only_registered_actions() {
    let scratch = Scratch::new("names");
    let (registry, _) = registry(&scratch);
    for id in [PARENT, CHILD] {
        let flow = shipped(id);
        for step in flow.graph.steps() {
            assert!(
                registry.get(&step.action).is_some(),
                "«{id}» names «{}» at step «{}», which nothing registers",
                step.action,
                step.id
            );
        }
    }
    let child = shipped(CHILD);
    let roots: Vec<&str> = child
        .graph
        .steps()
        .iter()
        .filter(|step| step.deps.is_empty())
        .map(|step| step.action.as_str())
        .collect();
    assert_eq!(
        roots,
        vec![actions::bench::build::BENCH_VALIDATE_ACTION],
        "the candidate is the root step's input, so the root step is the validation"
    );
}

// ── the repository of this test's own ────────────────────────────────────

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .expect("git starts");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(repo: &Path, relative: &str, text: &str) {
    let path = repo.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the file's directory");
    std::fs::write(path, text).expect("the fixture file");
}

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["-c", "commit.gpgsign=false", "commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

/// A one-function crate with a bug, then the fix that adds the test proving
/// it: the one candidate the flow will find.
fn a_repository_with_one_fix(root: &Path) -> (PathBuf, String) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("the repository's directory");
    git(&repo, &["init", "-q"]);
    write(&repo, "Cargo.toml", "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n");
    write(&repo, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n");
    write(&repo, ".gitignore", "target/\nCargo.lock\n");
    commit(&repo, "feat: the adder\n\nOne function, so a test has something to call.");
    write(&repo, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
    write(&repo, "tests/adds.rs", "#[test]\nfn two_and_two_make_four() {\n    assert_eq!(fixture::add(2, 2), 4);\n}\n");
    let fix = commit(&repo, "fix(adder): two and two make four\n\nThe adder subtracted what it was asked to add.\n");
    (repo, fix)
}

/// **THE FLOW RUNS AND THE SET IS FROZEN.** The trigger's text names the
/// repository, the child validates the one candidate against its base, the
/// task lands under the home, and the store holds the set's hash and ids.
#[test]
fn a_run_over_a_repository_with_one_fix_writes_one_task_and_records_the_set() {
    let scratch = Scratch::new("run");
    let (repo, fix) = a_repository_with_one_fix(&scratch.0);
    let (registry, ledger) = registry(&scratch);

    let flow = shipped(PARENT);
    let mut root_inputs = flow.inputs.clone();
    root_inputs.get_mut("trigger").expect("the trigger's input")["text"] = json!({
        "repo": repo.display().to_string(),
        "since": "2000-01-01",
        "limit": 10,
        "set": "stage-test",
    })
    .to_string()
    .into();
    let mut shared = SharedState::new();
    shared.insert(flow::WORKSPACE_ROOT.to_owned(), scratch.0.display().to_string().into());
    let store = InMemoryRecordStore::default();
    let execution = InProcessExecutor
        .execute(
            &flow.graph,
            ExecutionRequest {
                holder: None,
                run_id: "bench-run".to_owned(),
                root_inputs,
                gates: Vec::new(),
                shared,
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &registry,
            &Tick(0.into()),
        )
        .expect("the execution does not break");
    let records = flow::RecordStore::records(&store, "bench-run").expect("the steps");
    let broke: Vec<String> = records
        .iter()
        .filter(|record| record.outcome != Some(Outcome::Went))
        .map(|record| format!("{}: {:?} {:?} {:?} input={}", record.step_id, record.failure_class, record.said, record.refusal, record.input))
        .collect();
    // A child run's steps are in the ledger: the parent's store holds only the
    // step that opened it, so the reason is fetched from where the child wrote.
    let children: Vec<String> = broke
        .iter()
        .filter_map(|line| line.split(" run ").nth(1).and_then(|rest| rest.split(" of flow").next()))
        .flat_map(|child| flow::RecordStore::records(&ledger, child).unwrap_or_default())
        .map(|record| format!("  child {}: {:?} {:?} {:?}", record.step_id, record.failure_class, record.said, record.refusal))
        .collect();
    assert_eq!(
        flow::run_status(&execution),
        ("complete", true),
        "{:?}\n{}\n{}",
        execution.decisions.last(),
        broke.join("\n"),
        children.join("\n")
    );
    let of = |step: &str| {
        records
            .iter()
            .find(|record| record.step_id == step)
            .unwrap_or_else(|| panic!("the step «{step}» ran"))
    };
    assert_eq!(of("candidates").outcome, Some(Outcome::Went));
    let candidates = of("candidates").output.clone().expect("what the reading said");
    assert_eq!(candidates["candidates"].as_array().map(Vec::len), Some(1), "{candidates}");
    assert_eq!(candidates["candidates"][0]["fix_commit"], fix);

    let id: String = fix.chars().take(8).collect();
    let task = actions::bench::task::Task::load(&actions::bench::task::Task::path_in(&scratch.0, "stage-test", &id))
        .expect("the task was written under the home");
    let validated = task.validated.expect("validated");
    assert_eq!((validated.red_on_base, validated.green_on_fix), (3, 3), "three times each, as the child says");
    assert_eq!(task.prompt, "The adder subtracted what it was asked to add.");

    let frozen = of("freeze").output.clone().expect("the hash");
    assert_eq!(frozen["n"], 1);
    assert_eq!(frozen["ids"], json!([id]));
    let recorded = ledger
        .read_record("bench-sets", "stage-test")
        .expect("the store reads")
        .expect("the set is recorded");
    assert_eq!(recorded.value["hash"], frozen["hash"]);
    assert_eq!(recorded.value["n"], 1);
    assert_eq!(recorded.written_by, PARENT);
}
