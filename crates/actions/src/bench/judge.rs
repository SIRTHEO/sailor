//! The judge of the evaluation loop: whether a change made against a task
//! compiles, passes the tests it never saw, and leaves the test files alone.
//! What it measures beyond the verdict — size against the gold patch, lint
//! noise, where the change landed — is recorded as debt and decides nothing.

use crate::bench::task::{
    added_lines, diff_touches_test_attributes, files_in_diff, is_test_path, Task,
};
use crate::process::{run_with_timeout, RunOutcome};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The name `JudgeChangeAction` registers under.
pub const JUDGE_CHANGE_ACTION: &str = "judge_change";

const KNOWN_FIELDS: &[&str] = &[
    "tree",
    "task_file",
    "task",
    "run_status",
    "target_dir",
    "timeout_secs",
    "clippy",
    "launch",
    "workdir",
];

const DEFAULT_TIMEOUT_SECS: u64 = 1800;
/// Inside `target/`, so `cargo clean` takes it back and git never sees it.
const DEFAULT_TARGET_DIR: &str = "target/judge";
const HIDDEN_PATCH_FILE: &str = "hidden-tests.patch";
const HIDDEN_LOG_FILE: &str = "hidden-tests.log";
const TAIL_CHARS: usize = 4000;
const BLOAT_FACTOR: u64 = 3;
/// A gold patch of nothing would make every change bloat: it is compared to
/// this many lines instead.
const BLOAT_FLOOR_LINES: u64 = 30;
const COMPILERS: &str = "2";
const A_RUN_THAT_SAID_DONE: &str = "complete";

pub fn register_judge(registry: &mut flow::ActionRegistry) {
    registry.register(JUDGE_CHANGE_ACTION, JudgeChangeAction);
}

/// What a step writes, and also the shape of the launch text a trigger
/// carries: the same fields, read once from outside and once from inside.
#[derive(Debug, Default, Deserialize)]
struct JudgeSpec {
    #[serde(default)]
    tree: Option<String>,
    #[serde(default)]
    task_file: Option<String>,
    #[serde(default)]
    task: Option<Task>,
    #[serde(default)]
    run_status: Option<String>,
    #[serde(default)]
    target_dir: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    clippy: Option<bool>,
    #[serde(default)]
    launch: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
}

struct Asked {
    tree: PathBuf,
    task: Task,
    run_status: Option<String>,
    target_dir: PathBuf,
    timeout: Duration,
    clippy: bool,
}

fn invalid(said: impl Into<String>) -> ActionError {
    ActionError::new("invalid_input", said)
}

fn asked(input: &Value) -> Result<Asked, ActionError> {
    let written: JudgeSpec =
        serde_json::from_value(input.clone()).map_err(|error| invalid(error.to_string()))?;
    let carried = match written.launch.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => {
            serde_json::from_str::<JudgeSpec>(text).map_err(|error| {
                invalid(format!(
                    "the launch text is not a JSON object with tree, task_file and run_status: {error}"
                ))
            })?
        }
        _ => JudgeSpec::default(),
    };
    let tree = written
        .tree
        .or(carried.tree)
        .or(written.workdir)
        .map(PathBuf::from)
        .ok_or_else(|| invalid("a change is judged in a tree: write `tree`"))?;
    let task = match (written.task, written.task_file.or(carried.task_file)) {
        (Some(task), _) => task,
        (None, Some(path)) => {
            Task::load(Path::new(&path)).map_err(|why| ActionError::new("task_unreadable", why))?
        }
        (None, None) => {
            return Err(invalid(
                "a change is judged against a task: write `task_file` or `task`",
            ))
        }
    };
    if task.test_command.is_empty() {
        return Err(ActionError::new(
            "task_unreadable",
            format!("task «{}» names no test command", task.id),
        ));
    }
    let target_dir = written
        .target_dir
        .or(carried.target_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_TARGET_DIR));
    let target_dir = if target_dir.is_absolute() {
        target_dir
    } else {
        tree.join(target_dir)
    };
    Ok(Asked {
        tree,
        task,
        run_status: written.run_status.or(carried.run_status),
        target_dir,
        timeout: Duration::from_secs(
            written
                .timeout_secs
                .or(carried.timeout_secs)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        ),
        clippy: written.clippy.or(carried.clippy).unwrap_or(true),
    })
}

/// Everything the judge measured. `tree_left` is written only when the tree
/// was not restored: a key present and empty would read downstream as a fact.
#[derive(Debug, Default, Serialize)]
struct Reading {
    accepted: bool,
    false_done: bool,
    failure_class: &'static str,
    compiles: bool,
    hidden_tests_applied: bool,
    hidden_tests_passed: bool,
    touches_tests: bool,
    touched_test_files: Vec<String>,
    changed_files: Vec<String>,
    gold_files: Vec<String>,
    overlap: Vec<String>,
    added_lines: u64,
    gold_added_lines: u64,
    bloat: bool,
    clippy_warnings: Option<u64>,
    clippy_said: String,
    test_tail: String,
    tree_restored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tree_left: Option<String>,
    seconds: u64,
}

fn git(tree: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(tree)
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn git_failed(why: String) -> ActionError {
    ActionError::new("git_failed", why)
}

fn is_under(path: &str, own: Option<&Path>) -> bool {
    own.is_some_and(|own| Path::new(path).starts_with(own))
}

/// The tree as `git status` sees it, the judge's own build directory left out.
fn tree_status(tree: &Path, own: Option<&Path>) -> Result<Vec<String>, ActionError> {
    Ok(
        git(tree, &["status", "--porcelain", "--untracked-files=all"])
            .map_err(git_failed)?
            .lines()
            .filter(|line| !line.get(3..).is_some_and(|path| is_under(path, own)))
            .map(str::to_owned)
            .collect(),
    )
}

/// The diff of the tree against the task's base, new files included: they are
/// added with intent and taken out of the index again once read.
fn read_change(tree: &Path, base: &str, own: Option<&Path>) -> Result<String, ActionError> {
    let untracked: Vec<String> = git(tree, &["ls-files", "--others", "--exclude-standard"])
        .map_err(git_failed)?
        .lines()
        .filter(|line| !line.is_empty() && !is_under(line, own))
        .map(str::to_owned)
        .collect();
    if !untracked.is_empty() {
        let mut args = vec!["add", "--intent-to-add", "--"];
        args.extend(untracked.iter().map(String::as_str));
        git(tree, &args).map_err(git_failed)?;
    }
    let diff = git(tree, &["diff", "--no-color", "--no-ext-diff", base]);
    if !untracked.is_empty() {
        let mut args = vec!["reset", "-q", "--"];
        args.extend(untracked.iter().map(String::as_str));
        git(tree, &args).map_err(git_failed)?;
    }
    diff.map_err(git_failed)
}

struct HiddenTests {
    applied: bool,
    compiles: bool,
    passed: bool,
    tail: String,
    restored: Result<(), String>,
}

fn tail_of(text: &str) -> String {
    let start = text
        .char_indices()
        .rev()
        .nth(TAIL_CHARS - 1)
        .map_or(0, |(at, _)| at);
    text[start..].to_owned()
}

/// The task's command, `-j` added when it is cargo and before any `--`, which
/// hands what follows to the test binary.
fn test_command(task: &Task, tree: &Path, target_dir: &Path) -> Option<Command> {
    let (program, rest) = task.test_command.split_first()?;
    let mut command = Command::new(program);
    let cut = rest
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(rest.len());
    command.args(&rest[..cut]);
    if Path::new(program)
        .file_name()
        .is_some_and(|name| name == "cargo")
    {
        command.args(["-j", COMPILERS]);
    }
    command.args(&rest[cut..]);
    command
        .current_dir(tree)
        .env("CARGO_TARGET_DIR", target_dir);
    Some(command)
}

fn run_hidden_tests(asked: &Asked) -> Result<HiddenTests, ActionError> {
    let tree = &asked.tree;
    let patch = asked.target_dir.join(HIDDEN_PATCH_FILE);
    std::fs::write(&patch, &asked.task.hidden_test_patch).map_err(|error| {
        ActionError::new(
            "target_dir_unwritable",
            format!("{}: {error}", patch.display()),
        )
    })?;
    let patch_path = patch.display().to_string();
    let not_applied = |why: String| HiddenTests {
        applied: false,
        compiles: false,
        passed: false,
        tail: why,
        restored: Ok(()),
    };
    if let Err(why) = git(tree, &["apply", "--check", &patch_path]) {
        return Ok(not_applied(why));
    }
    if let Err(why) = git(tree, &["apply", &patch_path]) {
        return Ok(not_applied(why));
    }
    let Some(command) = test_command(&asked.task, tree, &asked.target_dir) else {
        return Err(invalid("the task names no test command"));
    };
    let outcome = run_with_timeout(command, asked.timeout);
    let restored = git(tree, &["apply", "-R", &patch_path]).map(|_| ());
    let (compiles, passed, tail) = match outcome {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let mut text = String::from_utf8_lossy(&stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&stderr));
            let log = asked.target_dir.join(HIDDEN_LOG_FILE);
            if let Err(error) = std::fs::write(&log, &text) {
                text.push_str(&format!("\n[judge] {}: {error}", log.display()));
            }
            let compiles = !text.contains("error: could not compile")
                && !text.contains("error[E")
                && (status.success() || text.contains("test result:"));
            (compiles, status.success(), tail_of(&text))
        }
        RunOutcome::TimedOut => (
            false,
            false,
            format!(
                "the hidden tests did not finish within {} seconds and were killed",
                asked.timeout.as_secs()
            ),
        ),
        RunOutcome::SpawnFailed(why) => {
            return Err(ActionError::new(
                "test_command_not_runnable",
                format!("{}: {why}", asked.task.test_command.join(" ")),
            ))
        }
    };
    Ok(HiddenTests {
        applied: true,
        compiles,
        passed,
        tail,
        restored,
    })
}

fn package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = line
            .strip_prefix("name")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// The packages the changed Rust files belong to: the nearest `Cargo.toml`
/// above each that declares a `[package]`.
fn packages_of(tree: &Path, changed: &[String]) -> Vec<String> {
    let mut found = BTreeSet::new();
    for file in changed.iter().filter(|file| file.ends_with(".rs")) {
        let mut dir = Path::new(file).parent();
        while let Some(here) = dir {
            if let Some(name) = package_name(&tree.join(here).join("Cargo.toml")) {
                found.insert(name);
                break;
            }
            dir = here.parent();
        }
    }
    found.into_iter().collect()
}

fn warnings_in(text: &str) -> u64 {
    text.lines()
        .filter(|line| line.starts_with("warning:") && !line.contains(" generated "))
        .count() as u64
}

fn lint(asked: &Asked, changed: &[String]) -> (Option<u64>, String) {
    let packages = packages_of(&asked.tree, changed);
    if packages.is_empty() {
        return (
            None,
            "no Rust file of a package changed: nothing to lint".to_owned(),
        );
    }
    let mut command = Command::new("cargo");
    command.arg("clippy");
    for package in &packages {
        command.args(["-p", package]);
    }
    command
        .args(["--tests", "-j", COMPILERS])
        .current_dir(&asked.tree)
        .env("CARGO_TARGET_DIR", &asked.target_dir);
    match run_with_timeout(command, asked.timeout) {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let mut text = String::from_utf8_lossy(&stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&stdout));
            if status.success() {
                (
                    Some(warnings_in(&text)),
                    format!("linted {}", packages.join(", ")),
                )
            } else {
                (None, tail_of(&text))
            }
        }
        RunOutcome::TimedOut => (
            None,
            format!(
                "clippy did not finish within {} seconds",
                asked.timeout.as_secs()
            ),
        ),
        RunOutcome::SpawnFailed(why) => (None, why),
    }
}

fn what_changed(before: &[String], after: &[String]) -> String {
    let gone: Vec<&str> = before
        .iter()
        .filter(|line| !after.contains(line))
        .map(String::as_str)
        .collect();
    let new: Vec<&str> = after
        .iter()
        .filter(|line| !before.contains(line))
        .map(String::as_str)
        .collect();
    format!("status lines gone: {gone:?}; status lines new: {new:?}")
}

/// One word, by precedence. The two classes read off the hidden tests are
/// reached only when those tests ran: a patch that did not apply measured
/// neither a compile nor a failure.
fn class_of(reading: &Reading) -> &'static str {
    if reading.accepted {
        "accepted"
    } else if reading.touches_tests {
        "validation_retreat"
    } else if reading.changed_files.is_empty() {
        "no_change"
    } else if reading.hidden_tests_applied && !reading.compiles {
        "does_not_compile"
    } else if reading.overlap.is_empty() {
        "localization_miss"
    } else if reading.hidden_tests_applied {
        "incomplete_repair"
    } else {
        "hidden_tests_not_applicable"
    }
}

fn judge(asked: &Asked) -> Result<Reading, ActionError> {
    let started = Instant::now();
    let tree = &asked.tree;
    git(tree, &["rev-parse", "--show-toplevel"])
        .map_err(|why| ActionError::new("not_a_git_tree", format!("{}: {why}", tree.display())))?;
    let base = asked.task.base_commit.as_str();
    git(tree, &["cat-file", "-e", &format!("{base}^{{commit}}")]).map_err(|why| {
        ActionError::new(
            "base_commit_unknown",
            format!("task «{}»: {why}", asked.task.id),
        )
    })?;
    std::fs::create_dir_all(&asked.target_dir).map_err(|error| {
        ActionError::new(
            "target_dir_unwritable",
            format!("{}: {error}", asked.target_dir.display()),
        )
    })?;
    let own = asked.target_dir.strip_prefix(tree).ok();

    let status_before = tree_status(tree, own)?;
    let diff = read_change(tree, base, own)?;
    let changed_files = files_in_diff(&diff);
    let touched_test_files: Vec<String> = changed_files
        .iter()
        .filter(|file| is_test_path(file))
        .cloned()
        .collect();
    let touches_tests = !touched_test_files.is_empty() || diff_touches_test_attributes(&diff);

    let hidden = run_hidden_tests(asked)?;

    let gold_files: Vec<String> = files_in_diff(&asked.task.gold_patch)
        .into_iter()
        .filter(|file| !is_test_path(file))
        .collect();
    let overlap: Vec<String> = changed_files
        .iter()
        .filter(|file| !is_test_path(file) && gold_files.contains(file))
        .cloned()
        .collect();
    let added = added_lines(&diff);
    let gold_added = asked.task.gold_added_lines;
    let bloat_past = if gold_added == 0 {
        BLOAT_FLOOR_LINES
    } else {
        BLOAT_FACTOR * gold_added
    };
    let (clippy_warnings, clippy_said) = if asked.clippy {
        lint(asked, &changed_files)
    } else {
        (None, "not asked".to_owned())
    };

    let status_after = tree_status(tree, own)?;
    let tree_left = match &hidden.restored {
        Err(why) => Some(why.clone()),
        Ok(()) if status_after != status_before => {
            Some(what_changed(&status_before, &status_after))
        }
        Ok(()) => None,
    };

    let accepted = hidden.compiles && hidden.applied && hidden.passed && !touches_tests;
    let mut reading = Reading {
        accepted,
        false_done: asked.run_status.as_deref() == Some(A_RUN_THAT_SAID_DONE) && !accepted,
        failure_class: "",
        compiles: hidden.compiles,
        hidden_tests_applied: hidden.applied,
        hidden_tests_passed: hidden.passed,
        touches_tests,
        touched_test_files,
        changed_files,
        gold_files,
        overlap,
        added_lines: added,
        gold_added_lines: gold_added,
        bloat: added > bloat_past,
        clippy_warnings,
        clippy_said,
        test_tail: hidden.tail,
        tree_restored: tree_left.is_none(),
        tree_left,
        seconds: started.elapsed().as_secs(),
    };
    reading.failure_class = class_of(&reading);
    Ok(reading)
}

/// Accepts or rejects a change against a task, deterministically and with
/// every check recorded whether or not an earlier one failed.
pub struct JudgeChangeAction;

impl Action for JudgeChangeAction {
    /// Builds and tests on this machine, and buys nothing.
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let reading = judge(&asked(input)?)?;
        serde_json::to_value(reading)
            .map(ActionOutcome::Went)
            .map_err(|error| ActionError::new("reading_unserialisable", error.to_string()))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LIB_WRONG: &str = "pub fn answer() -> u32 {\n    41\n}\n";
    const LIB_RIGHT: &str = "pub fn answer() -> u32 {\n    42\n}\n";
    const LIB_STILL_WRONG: &str = "pub fn answer() -> u32 {\n    43\n}\n";
    const LIB_NOT_RUST: &str = "pub fn answer() -> u32 {\n    \"42\"\n}\n";
    const HIDDEN_TEST: &str =
        "#[test]\nfn the_answer_is_right() {\n    assert_eq!(tiny::answer(), 42);\n}\n";
    const MANIFEST: &str =
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n";

    /// A tiny crate in a repository of its own: one commit with the wrong
    /// answer, a gold patch that fixes it, and a hidden test asking for it.
    struct Fixture {
        dir: PathBuf,
        repo: PathBuf,
        task_file: PathBuf,
        task: Task,
    }

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let dir =
                std::env::temp_dir().join(format!("sailor-judge-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let repo = dir.join("tiny");
            std::fs::create_dir_all(repo.join("src")).expect("a crate to judge");
            std::fs::write(repo.join("Cargo.toml"), MANIFEST).expect("manifest");
            std::fs::write(repo.join(".gitignore"), "target\nCargo.lock\n").expect("gitignore");
            std::fs::write(repo.join("src/lib.rs"), LIB_WRONG).expect("lib");
            let sh = |args: &[&str]| git(&repo, args).expect("git in the fixture");
            sh(&["init", "-q"]);
            sh(&["add", "-A"]);
            sh(&[
                "-c",
                "user.name=judge",
                "-c",
                "user.email=judge@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "base",
            ]);
            let base = sh(&["rev-parse", "HEAD"]).trim().to_owned();
            std::fs::write(repo.join("src/lib.rs"), LIB_RIGHT).expect("gold");
            let gold_patch = sh(&["diff"]);
            sh(&["checkout", "-q", "--", "src/lib.rs"]);
            std::fs::create_dir_all(repo.join("tests")).expect("tests dir");
            std::fs::write(repo.join("tests/hidden.rs"), HIDDEN_TEST).expect("hidden");
            sh(&["add", "-N", "tests/hidden.rs"]);
            let hidden_test_patch = sh(&["diff"]);
            sh(&["reset", "-q", "--", "tests/hidden.rs"]);
            std::fs::remove_dir_all(repo.join("tests")).expect("hidden test taken out");
            let task = Task {
                id: "tiny-answer".into(),
                repo: repo.display().to_string(),
                base_commit: base.clone(),
                fix_commit: base,
                prompt: "answer() returns 41".into(),
                prompt_source: "fixture".into(),
                test_files: vec!["tests/hidden.rs".into()],
                hidden_test_patch,
                gold_added_lines: added_lines(&gold_patch),
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
                task,
            }
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.repo.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("a directory in the fixture");
            }
            std::fs::write(path, text).expect("a file in the fixture");
        }

        fn reset(&self) {
            git(&self.repo, &["checkout", "-q", "--", "."]).expect("checkout");
            git(&self.repo, &["clean", "-qfd"]).expect("clean");
        }

        fn status(&self) -> String {
            git(
                &self.repo,
                &["status", "--porcelain", "--untracked-files=all"],
            )
            .expect("status")
        }

        fn input(&self) -> Value {
            json!({
                "tree": self.repo.display().to_string(),
                "task_file": self.task_file.display().to_string(),
                "clippy": false,
            })
        }

        fn judge(&self, extra: Value) -> Value {
            let mut input = self.input();
            for (key, value) in extra.as_object().expect("extra fields are an object") {
                input[key] = value.clone();
            }
            let outcome = JudgeChangeAction
                .execute(&input, &SharedState::new())
                .expect("the judge reads a tree it was handed");
            let ActionOutcome::Went(reading) = outcome else {
                panic!("a judge that read is always Went: {outcome:?}")
            };
            reading
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn strings(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(Value::as_str)
            .collect()
    }

    /// Red first with `compiles` forced false, with the `apply -R` skipped
    /// (`tree_restored` and the status compare both fell), and with
    /// `false_done` computed without `accepted`.
    #[test]
    fn the_gold_patch_is_accepted_and_the_tree_is_left_as_found() {
        let fixture = Fixture::new("gold");
        fixture.write("src/lib.rs", LIB_RIGHT);
        let before = fixture.status();

        let reading = fixture.judge(json!({"run_status": "complete"}));

        assert_eq!(reading["accepted"], true, "{reading}");
        assert_eq!(reading["failure_class"], "accepted");
        assert_eq!(reading["false_done"], false);
        assert_eq!(reading["compiles"], true);
        assert_eq!(reading["hidden_tests_applied"], true);
        assert_eq!(reading["hidden_tests_passed"], true);
        assert_eq!(reading["touches_tests"], false);
        assert_eq!(strings(&reading["changed_files"]), ["src/lib.rs"]);
        assert_eq!(strings(&reading["gold_files"]), ["src/lib.rs"]);
        assert_eq!(strings(&reading["overlap"]), ["src/lib.rs"]);
        assert_eq!(reading["added_lines"], reading["gold_added_lines"]);
        assert_eq!(reading["gold_added_lines"], fixture.task.gold_added_lines);
        assert_eq!(reading["bloat"], false);
        assert!(
            reading["clippy_warnings"].is_null(),
            "clippy was not asked: {reading}"
        );
        assert_eq!(reading["tree_restored"], true, "{reading}");
        assert!(reading.get("tree_left").is_none(), "{reading}");
        assert!(
            reading["test_tail"]
                .as_str()
                .expect("a tail")
                .contains("test result: ok"),
            "{}",
            reading["test_tail"]
        );
        assert!(
            !fixture.repo.join("tests/hidden.rs").exists(),
            "the hidden test stayed behind"
        );
        assert_eq!(fixture.status(), before, "the tree is not as it was found");
    }

    /// Red first with `compiles` forced true.
    #[test]
    fn a_change_that_does_not_compile_is_rejected_for_that() {
        let fixture = Fixture::new("no-compile");
        fixture.write("src/lib.rs", LIB_NOT_RUST);

        let reading = fixture.judge(json!({"run_status": "complete"}));

        assert_eq!(reading["compiles"], false, "{reading}");
        assert_eq!(reading["accepted"], false);
        assert_eq!(reading["hidden_tests_passed"], false);
        assert_eq!(reading["failure_class"], "does_not_compile");
        assert_eq!(reading["false_done"], true);
        assert!(
            reading["test_tail"]
                .as_str()
                .expect("a tail")
                .contains("error[E"),
            "{}",
            reading["test_tail"]
        );
        assert_eq!(reading["tree_restored"], true, "{reading}");
    }

    /// Red first with `hidden_tests_passed` read as `compiles` instead of the
    /// exit status, and with `overlap` computed as the gold files alone.
    #[test]
    fn a_change_that_compiles_but_leaves_the_value_wrong_is_an_incomplete_repair() {
        let fixture = Fixture::new("still-wrong");
        fixture.write("src/lib.rs", LIB_STILL_WRONG);

        let reading = fixture.judge(json!({"run_status": "complete"}));

        assert_eq!(reading["compiles"], true, "{reading}");
        assert_eq!(reading["hidden_tests_passed"], false);
        assert_eq!(reading["accepted"], false);
        assert_eq!(reading["failure_class"], "incomplete_repair");
        assert_eq!(reading["false_done"], true);
        assert_eq!(strings(&reading["overlap"]), ["src/lib.rs"]);

        fixture.reset();
        fixture.write("NOTES.md", "a note beside the code\n");

        let reading = fixture.judge(json!({}));

        assert_eq!(
            strings(&reading["changed_files"]),
            ["NOTES.md"],
            "{reading}"
        );
        assert!(strings(&reading["overlap"]).is_empty(), "{reading}");
        assert_eq!(reading["compiles"], true);
        assert_eq!(reading["hidden_tests_passed"], false);
        assert_eq!(reading["failure_class"], "localization_miss");
        assert_eq!(reading["false_done"], false, "no run status was given");
        assert_eq!(reading["tree_restored"], true, "{reading}");
    }

    /// Red first with `touches_tests` forced false: the hidden tests pass in
    /// both halves, and only this check says no.
    #[test]
    fn a_change_that_touches_the_tests_is_a_validation_retreat_even_when_they_pass() {
        let fixture = Fixture::new("retreat");
        fixture.write("src/lib.rs", LIB_RIGHT);
        fixture.write("tests/other.rs", "#[test]\nfn always() {}\n");

        let reading = fixture.judge(json!({}));

        assert_eq!(reading["hidden_tests_passed"], true, "{reading}");
        assert_eq!(reading["compiles"], true);
        assert_eq!(reading["touches_tests"], true);
        assert_eq!(strings(&reading["touched_test_files"]), ["tests/other.rs"]);
        assert_eq!(reading["accepted"], false);
        assert_eq!(reading["failure_class"], "validation_retreat");

        fixture.reset();
        fixture.write(
            "src/lib.rs",
            &format!("{LIB_RIGHT}\n#[cfg(test)]\nmod tests {{}}\n"),
        );

        let reading = fixture.judge(json!({}));

        assert_eq!(reading["hidden_tests_passed"], true, "{reading}");
        assert_eq!(
            reading["touches_tests"], true,
            "a test attribute in a non-test file"
        );
        assert!(strings(&reading["touched_test_files"]).is_empty());
        assert_eq!(reading["failure_class"], "validation_retreat");
    }

    /// Red first with `false_done` ignoring the run status.
    #[test]
    fn a_rejected_change_is_a_false_done_only_when_its_run_said_complete() {
        let fixture = Fixture::new("false-done");
        fixture.write("src/lib.rs", LIB_STILL_WRONG);

        let reading = fixture.judge(json!({"run_status": "failed"}));

        assert_eq!(reading["accepted"], false, "{reading}");
        assert_eq!(reading["false_done"], false);
    }

    /// Red first with `bloat` forced false, and with the lint count read from
    /// a change with no Rust file.
    #[test]
    fn a_bloated_change_is_accepted_and_its_debt_is_recorded() {
        let fixture = Fixture::new("bloat");
        fixture.write("src/lib.rs", &format!("pub mod padding;\n{LIB_RIGHT}"));
        let padding: String = (0..200).map(|n| format!("pub fn p{n}() {{}}\n")).collect();
        fixture.write("src/padding.rs", &padding);

        let reading = fixture.judge(json!({"clippy": true}));

        assert_eq!(reading["accepted"], true, "{reading}");
        assert_eq!(reading["failure_class"], "accepted");
        assert_eq!(reading["bloat"], true);
        assert!(
            reading["added_lines"].as_u64().expect("a count") > 200,
            "{reading}"
        );
        assert_eq!(reading["gold_added_lines"], 1);
        assert!(
            reading["clippy_warnings"].is_u64(),
            "clippy was asked and counted: {reading}"
        );
        assert!(
            reading["clippy_said"]
                .as_str()
                .expect("a word")
                .contains("tiny"),
            "{reading}"
        );
        assert_eq!(reading["tree_restored"], true, "{reading}");
    }

    /// Red first with `hidden_tests_applied` forced true.
    #[test]
    fn a_hidden_patch_that_does_not_apply_is_a_rejection_of_its_own_class() {
        let fixture = Fixture::new("not-applicable");
        fixture.write("src/lib.rs", LIB_RIGHT);
        let mut task = fixture.task.clone();
        task.hidden_test_patch = "this is not a patch\n".to_owned();
        let task_file = fixture.dir.join("garbled.json");
        task.save(&task_file).expect("the garbled task is written");

        let reading = fixture.judge(json!({"task_file": task_file.display().to_string()}));

        assert_eq!(reading["hidden_tests_applied"], false, "{reading}");
        assert_eq!(reading["hidden_tests_passed"], false);
        assert_eq!(reading["compiles"], false);
        assert_eq!(reading["accepted"], false);
        assert_eq!(reading["failure_class"], "hidden_tests_not_applicable");
        assert!(!reading["test_tail"].as_str().expect("a why").is_empty());
        assert_eq!(reading["tree_restored"], true, "{reading}");
    }

    /// Red first with the launch text ignored, and with the carried field
    /// winning over the written one.
    #[test]
    fn the_launch_text_fills_what_the_step_did_not_write_and_never_overrides_it() {
        let fixture = Fixture::new("launch");
        fixture.write("src/lib.rs", LIB_STILL_WRONG);
        let launch = json!({
            "tree": fixture.repo.display().to_string(),
            "task_file": fixture.task_file.display().to_string(),
            "run_status": "complete",
            "clippy": false,
        })
        .to_string();

        let ActionOutcome::Went(reading) = JudgeChangeAction
            .execute(&json!({"launch": launch}), &SharedState::new())
            .expect("the launch text names the tree and the task")
        else {
            panic!("a judge that read is always Went")
        };
        assert_eq!(reading["failure_class"], "incomplete_repair", "{reading}");
        assert_eq!(reading["false_done"], true);

        let ActionOutcome::Went(reading) = JudgeChangeAction
            .execute(
                &json!({"launch": launch, "run_status": "failed"}),
                &SharedState::new(),
            )
            .expect("the written field stands beside the launch")
        else {
            panic!("a judge that read is always Went")
        };
        assert_eq!(
            reading["false_done"], false,
            "the written field wins: {reading}"
        );
    }

    #[test]
    fn the_judge_refuses_what_it_cannot_measure_and_names_why() {
        let fixture = Fixture::new("refusals");
        let class = |input: Value| {
            JudgeChangeAction
                .execute(&input, &SharedState::new())
                .expect_err("nothing here can be measured")
                .class
        };
        assert_eq!(
            class(json!({"task_file": fixture.task_file.display().to_string()})),
            "invalid_input"
        );
        assert_eq!(
            class(json!({"tree": fixture.repo.display().to_string()})),
            "invalid_input"
        );
        assert_eq!(
            class(json!({
                "tree": fixture.dir.display().to_string(),
                "task_file": fixture.task_file.display().to_string()
            })),
            "not_a_git_tree"
        );
        assert_eq!(
            class(json!({
                "tree": fixture.repo.display().to_string(),
                "task_file": fixture.dir.join("missing.json").display().to_string()
            })),
            "task_unreadable"
        );
        let mut elsewhere = fixture.task.clone();
        elsewhere.base_commit = "0123456789abcdef0123456789abcdef01234567".to_owned();
        assert_eq!(
            class(json!({"tree": fixture.repo.display().to_string(), "task": elsewhere})),
            "base_commit_unknown"
        );
        assert_eq!(
            class(json!({"launch": "not json", "tree": "x"})),
            "invalid_input"
        );
    }

    #[test]
    fn the_fields_the_judge_knows_are_the_ones_it_reads() {
        let declared = json!({"tree": "t", "task_file": "f", "run_status": "complete", "clippy": false, "typo": 1});
        assert_eq!(JudgeChangeAction.unknown_fields(&declared), vec!["typo"]);
        assert!(JudgeChangeAction
            .unknown_fields(&json!("not an object"))
            .is_empty());
        assert!(!JudgeChangeAction.may_spend(None));
    }

    #[test]
    fn a_test_command_is_told_how_many_compilers_before_what_it_hands_on() {
        let task = Task {
            test_command: ["cargo", "test", "-p", "x", "--", "--nocapture"]
                .iter()
                .map(|word| word.to_string())
                .collect(),
            ..Fixture::new("argv").task.clone()
        };
        let command =
            test_command(&task, Path::new("."), Path::new("target/judge")).expect("a command");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            ["test", "-p", "x", "-j", COMPILERS, "--", "--nocapture"]
        );
        assert_eq!(package_name(Path::new("/nowhere/Cargo.toml")), None);
        assert_eq!(
            warnings_in("warning: unused\nwarning: `x` (lib) generated 1 warning\nerror: no\n"),
            1
        );
        assert_eq!(tail_of("abc").as_str(), "abc");
    }
}
