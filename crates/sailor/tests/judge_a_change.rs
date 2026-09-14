//! The shipped `judge-a-change`: a change against a benchmark task is judged
//! by code, and the run is red exactly when the change is rejected. No engine
//! runs here; the tree judged is a tiny crate of the test's own.

use flow::system::{load_all, FlowSource};
use flow::{
    Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, SharedState,
};
use serde_json::json;
use std::path::{Path, PathBuf};

const FLOW_ID: &str = "judge-a-change";

const LIB_WRONG: &str = "pub fn answer() -> u32 {\n    41\n}\n";
const LIB_RIGHT: &str = "pub fn answer() -> u32 {\n    42\n}\n";
const LIB_STILL_WRONG: &str = "pub fn answer() -> u32 {\n    43\n}\n";
const HIDDEN_TEST: &str =
    "#[test]\nfn the_answer_is_right() {\n    assert_eq!(tiny::answer(), 42);\n}\n";
const MANIFEST: &str =
    "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";

fn shipped() -> FlowFile {
    load_all(&[FlowSource::builtin()])
        .into_iter()
        .find(|(name, _, _)| name == FLOW_ID)
        .map(|(_, _, entry)| entry.expect("the shipped flow loads"))
        .expect("the flow is shipped")
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A tiny crate in a repository of its own, with the task file beside it.
struct Fixture {
    dir: PathBuf,
    repo: PathBuf,
    task_file: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("sailor-judge-flow-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("tiny");
        std::fs::create_dir_all(repo.join("src")).expect("a crate to judge");
        std::fs::write(repo.join("Cargo.toml"), MANIFEST).expect("manifest");
        std::fs::write(repo.join(".gitignore"), "target\nCargo.lock\n").expect("gitignore");
        std::fs::write(repo.join("src/lib.rs"), LIB_WRONG).expect("lib");
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "-A"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=judge",
                "-c",
                "user.email=judge@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "base",
            ],
        );
        let base = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
        std::fs::write(repo.join("src/lib.rs"), LIB_RIGHT).expect("gold");
        let gold_patch = git(&repo, &["diff"]);
        git(&repo, &["checkout", "-q", "--", "src/lib.rs"]);
        std::fs::create_dir_all(repo.join("tests")).expect("tests dir");
        std::fs::write(repo.join("tests/hidden.rs"), HIDDEN_TEST).expect("hidden");
        git(&repo, &["add", "-N", "tests/hidden.rs"]);
        let hidden_test_patch = git(&repo, &["diff"]);
        git(&repo, &["reset", "-q", "--", "tests/hidden.rs"]);
        std::fs::remove_dir_all(repo.join("tests")).expect("hidden test taken out");
        let task = actions::bench::task::Task {
            id: "tiny-answer".into(),
            repo: repo.display().to_string(),
            base_commit: base.clone(),
            fix_commit: base,
            prompt: "answer() returns 41".into(),
            prompt_source: "fixture".into(),
            test_files: vec!["tests/hidden.rs".into()],
            hidden_test_patch,
            gold_added_lines: actions::bench::task::added_lines(&gold_patch),
            gold_patch,
            test_command: ["cargo", "test", "-p", "tiny", "--test", "hidden"]
                .iter()
                .map(|word| word.to_string())
                .collect(),
            validated: None,
        };
        let task_file = dir.join("task.json");
        task.save(&task_file).expect("the task is written");
        Fixture {
            dir,
            repo,
            task_file,
        }
    }

    fn change_to(&self, lib: &str) {
        std::fs::write(self.repo.join("src/lib.rs"), lib).expect("the change");
    }

    fn launch(&self, run_status: &str) -> String {
        json!({
            "tree": self.repo.display().to_string(),
            "task_file": self.task_file.display().to_string(),
            "run_status": run_status,
            "clippy": false,
        })
        .to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Tick(std::sync::atomic::AtomicI64);

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

/// The product's registry over a house of the fixture's own, as a real run
/// builds it; the launch enters as the trigger's text, the one input a launch sets.
fn run(fixture: &Fixture, launch: &str) -> (Execution, InMemoryRecordStore) {
    let ledger = ledger::Ledger::open(fixture.dir.join("ledger")).expect("a store of our own");
    let registry = registry::registry_in(registry::House::under(&fixture.dir), Some(ledger), None);
    let flow = shipped();
    let mut root_inputs: std::collections::BTreeMap<String, serde_json::Value> =
        flow.inputs.into_iter().collect();
    root_inputs.get_mut("trigger").expect("the trigger's input")["text"] = json!(launch);
    let store = InMemoryRecordStore::default();
    let mut shared = SharedState::new();
    shared.insert(
        flow::WORKSPACE_ROOT.to_owned(),
        fixture.repo.display().to_string().into(),
    );
    let request = ExecutionRequest {
        holder: None,
        run_id: "judged".to_owned(),
        root_inputs,
        gates: Vec::new(),
        shared,
        spend_cap_micros: None,
        stops: flow::RunStops::default(),
    };
    let execution = InProcessExecutor
        .execute(&flow.graph, request, &store, &registry, &Tick(0.into()))
        .expect("the execution does not break");
    (execution, store)
}

/// The launch reaches the judge from the trigger's text, the verdict reads the
/// judge's own word, and the run cannot close without it.
#[test]
fn the_launch_reaches_the_judge_and_the_verdict_reads_its_word() {
    let flow = shipped();
    let steps: Vec<(&str, &str)> = flow
        .graph
        .steps()
        .iter()
        .map(|step| (step.id.as_str(), step.action.as_str()))
        .collect();
    assert_eq!(
        steps,
        vec![
            ("trigger", "trigger"),
            ("judge", actions::bench::judge::JUDGE_CHANGE_ACTION),
            ("verdict", actions::SHELL_CHECK_ACTION),
        ]
    );
    let judge = flow.graph.step("judge").expect("the judge step");
    assert_eq!(judge.deps, vec!["trigger"]);
    assert_eq!(
        judge.with.as_ref().expect("with")["launch"],
        json!({"$from": "/text"}),
        "the launch is the trigger's text"
    );
    let verdict = flow.graph.step("verdict").expect("the verdict step");
    assert_eq!(verdict.deps, vec!["judge"]);
    assert!(verdict.required, "a run without a verdict is not complete");
    let with = verdict.with.as_ref().expect("with");
    assert_eq!(with["env"]["ACCEPTED"], json!({"$json": "/accepted"}));
    assert_eq!(
        with["env"]["FAILURE_CLASS"],
        json!({"$from": "/failure_class"})
    );
    let registry = registry::registry_in(registry::House::empty(), None, None);
    for step in flow.graph.steps() {
        assert!(
            registry.get(&step.action).is_some(),
            "«{}» names «{}», which the product does not register",
            step.id,
            step.action
        );
    }
}

/// Red first with the verdict's command replaced by `true`: the rejected
/// change closed the run green, and only this test said so.
#[test]
fn a_rejected_change_ends_the_run_red_at_the_verdict_and_the_reading_is_kept() {
    let fixture = Fixture::new("rejected");
    fixture.change_to(LIB_STILL_WRONG);

    let (execution, store) = run(&fixture, &fixture.launch("complete"));

    let records = store.all();
    let judge = records
        .iter()
        .find(|record| record.step_id == "judge")
        .expect("the judge ran");
    assert_eq!(
        judge.outcome,
        Some(Outcome::Went),
        "{:?}",
        judge.failure_class
    );
    let reading = judge.output.as_ref().expect("the judge's reading is kept");
    assert_eq!(reading["accepted"], false, "{reading}");
    assert_eq!(reading["failure_class"], "incomplete_repair");
    assert_eq!(reading["false_done"], true);
    assert_eq!(reading["tree_restored"], true);
    let verdict = records
        .iter()
        .find(|record| record.step_id == "verdict")
        .expect("the verdict was asked");
    assert_eq!(
        verdict.outcome,
        Some(Outcome::Broke),
        "{:?}",
        verdict.failure_class
    );
    assert_eq!(verdict.failure_class.as_deref(), Some("check_failed"));
    assert_eq!(
        flow::run_status(&execution),
        ("failed", false),
        "{:?}",
        execution.decisions.last()
    );
}

/// The control: the gold change, and the same flow closes green.
#[test]
fn the_gold_change_closes_the_run_green() {
    let fixture = Fixture::new("gold");
    fixture.change_to(LIB_RIGHT);

    let (execution, store) = run(&fixture, &fixture.launch("complete"));

    let records = store.all();
    let judge = records
        .iter()
        .find(|record| record.step_id == "judge")
        .expect("the judge ran");
    let reading = judge.output.as_ref().expect("the judge's reading is kept");
    assert_eq!(reading["accepted"], true, "{reading}");
    assert_eq!(reading["false_done"], false);
    let verdict = records
        .iter()
        .find(|record| record.step_id == "verdict")
        .expect("the verdict was asked");
    assert_eq!(
        verdict.outcome,
        Some(Outcome::Went),
        "{:?}",
        verdict.failure_class
    );
    assert!(
        matches!(execution.decisions.last(), Some(Decision::Complete)),
        "{:?}",
        execution.decisions.last()
    );
    assert!(
        !fixture.repo.join("tests/hidden.rs").exists(),
        "the hidden test stayed behind"
    );
}
