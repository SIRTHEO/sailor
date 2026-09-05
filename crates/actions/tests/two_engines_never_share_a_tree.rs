//! A step that asks for a tree of its own runs in one, and a second step of
//! the same run gets another.
//!
//! **A REAL REPOSITORY AND A REAL PROCESS.** A test on the rule alone stays
//! green with the rule wired to nothing, which is fault 18: the engine here
//! prints the directory it was started in, and git cut the trees.

use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        // The clock alone is not a name: two tests of this binary run on two
        // threads and read the same microsecond, and then cut their trees in
        // one directory.
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = format!(
            "actions-trees-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("a directory to work in");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // The trees git cut sit beside the repository, so they go too.
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(self.0.join("repo"))
            .args(["worktree", "prune"])
            .output();
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        // The repository's own settings only: nothing of the runner's account.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
}

/// A repository with one commit: a worktree cannot be cut from a history that
/// has none.
fn a_repository_in(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).expect("the repository directory");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "prove@example"]);
    git(&repo, &["config", "user.name", "prove"]);
    fs::write(repo.join("README"), "a tree to cut from\n").expect("a file to commit");
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-q", "-m", "the first commit"]);
    repo
}

fn an_engine_that_prints_where_it_stands(dir: &Path) -> String {
    an_engine_called(dir, "dove", "pwd\n")
}

/// It reads the project's own file from where it stands, so the tree is
/// measured as a checkout while the step is alive, not by what survives it.
fn an_engine_that_reads_the_project(dir: &Path) -> String {
    an_engine_called(dir, "legge", "pwd\ncat README\n")
}

/// It leaves a file nobody committed, which is the state git refuses to lose.
fn an_engine_that_leaves_work_behind(dir: &Path) -> String {
    an_engine_called(dir, "lascia", "pwd\necho half a thought > left-behind\n")
}

fn an_engine_called(dir: &Path, name: &str, body: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}")).expect("write the engine");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
    path.to_string_lossy().into_owned()
}

/// What the person watching was told, kept so a test can read it back.
#[derive(Clone, Default)]
struct Overheard(Arc<Mutex<String>>);

impl Overheard {
    fn words(&self) -> String {
        self.0.lock().expect("the words").clone()
    }
}

impl actions::StepSinks for Overheard {
    fn sink_for(&self, _step: &str) -> Arc<dyn actions::LiveSink> {
        let heard = Arc::clone(&self.0);
        Arc::new(move |_pipe: actions::Pipe, bytes: &[u8]| {
            heard.lock().expect("the words").push_str(&String::from_utf8_lossy(bytes));
        })
    }
}

fn what_it_printed(outcome: &ActionOutcome) -> String {
    let ActionOutcome::Went(value) = outcome else {
        panic!("the step had to go: {outcome:?}");
    };
    value
        .get("stdout")
        .and_then(serde_json::Value::as_str)
        .expect("what the engine printed")
        .to_owned()
}

/// The first line, which is where the engine printed its own directory.
fn stood_in(outcome: &ActionOutcome) -> String {
    what_it_printed(outcome).lines().next().unwrap_or_default().to_owned()
}

fn shared_for(root: &Path, run: &str, step: &str) -> SharedState {
    let mut shared = SharedState::new();
    shared.insert(flow::WORKSPACE_ROOT.to_owned(), json!(root.to_string_lossy()));
    shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
    shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
    shared
}

#[test]
fn each_step_asking_for_a_tree_of_its_own_gets_one_and_nobody_shares() {
    let dir = TempDir::new();
    let repo = a_repository_in(dir.path());
    let bin = an_engine_that_prints_where_it_stands(dir.path());
    let reads = an_engine_that_reads_the_project(dir.path());
    let action = actions::ExternalEngineAction::new();
    let asks_for_a_tree = json!({"bin": reads, "tree": "own", "timeout_secs": 30});

    // The control: a step that asks for nothing stands where the run stands.
    let shared = shared_for(&repo, "corsa-1", "engine_a");
    let shared_tree = stood_in(
        &action
            .execute(&json!({"bin": bin, "timeout_secs": 30}), &shared)
            .expect("the step had to go"),
    );
    assert!(
        !shared_tree.contains("corsa-1"),
        "a step that asked for nothing was moved anyway: {shared_tree}"
    );

    let shared = shared_for(&repo, "corsa-1", "engine_a");
    let told = action.execute(&asks_for_a_tree, &shared).expect("the step had to go");
    let first = stood_in(&told);
    assert!(
        what_it_printed(&told).contains("a tree to cut from"),
        "the tree is a checkout of the project and not an empty directory: {first}"
    );
    let shared = shared_for(&repo, "corsa-1", "engine_b");
    let second = stood_in(&action.execute(&asks_for_a_tree, &shared).expect("the step had to go"));

    assert_ne!(first, second, "two steps of one run were given the same tree");
    for stood in [&first, &second] {
        assert!(stood.contains("corsa-1"), "the tree is named after the run: {stood}");
    }
    assert!(first.ends_with("engine_a") && second.ends_with("engine_b"), "{first} / {second}");

    // A step that answered and left nothing takes its tree with it: it is the
    // pairing that bounds the disk, so it is measured on git and on the disk.
    let listed = what_git_has_cut(&repo);
    assert!(
        !listed.contains(&first) && !listed.contains(&second),
        "git still holds the trees of two steps that left nothing:\n{listed}"
    );
    for stood in [&first, &second] {
        assert!(!Path::new(stood).exists(), "the tree is still on disk: {stood}");
    }
}

fn what_git_has_cut(repo: &Path) -> String {
    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "list"])
        .output()
        .expect("git lists what it cut");
    String::from_utf8_lossy(&listed.stdout).into_owned()
}

/// The refusal is the whole safety property: a step that left something not
/// committed keeps its tree, the person is told which tree and why, and the
/// next attempt lands in it and finds the work.
#[test]
fn a_step_that_leaves_work_keeps_its_tree_and_the_person_is_told() {
    let dir = TempDir::new();
    let repo = a_repository_in(dir.path());
    let bin = an_engine_that_leaves_work_behind(dir.path());
    let overheard = Overheard::default();
    let action = actions::ExternalEngineAction::new()
        .watched_by(Some(Arc::new(overheard.clone()) as Arc<dyn actions::StepSinks>));
    let asks_for_a_tree = json!({"bin": bin, "tree": "own", "timeout_secs": 30});

    let shared = shared_for(&repo, "corsa-3", "lascia");
    let kept = stood_in(&action.execute(&asks_for_a_tree, &shared).expect("the step had to go"));

    assert!(Path::new(&kept).join("left-behind").exists(), "the work was lost: {kept}");
    assert!(what_git_has_cut(&repo).contains(&kept), "git no longer holds the tree");
    let words = overheard.words();
    assert!(words.contains(&kept), "the kept tree was never named:\n{words}");
    assert!(words.contains("stays"), "nobody was told why it stays:\n{words}");

    // The next attempt finds what the one before it left.
    let shared = shared_for(&repo, "corsa-3", "lascia");
    let again = stood_in(&action.execute(&asks_for_a_tree, &shared).expect("the step had to go"));
    assert_eq!(again, kept, "a second attempt was cut a second tree");
}

#[test]
fn a_step_that_wants_a_tree_and_names_a_workdir_is_refused() {
    let dir = TempDir::new();
    let repo = a_repository_in(dir.path());
    let bin = an_engine_that_prints_where_it_stands(dir.path());
    let action = actions::ExternalEngineAction::new();

    let shared = shared_for(&repo, "corsa-2", "confuso");
    let refused = action
        .execute(
            &json!({"bin": bin, "tree": "own", "workdir": "/altrove", "timeout_secs": 30}),
            &shared,
        )
        .expect_err("two places for one step");
    assert_eq!(refused.class, "invalid_input", "{refused:?}");

    let shared = shared_for(&repo, "corsa-2", "sconosciuto");
    let unknown = action
        .execute(&json!({"bin": bin, "tree": "shared", "timeout_secs": 30}), &shared)
        .expect_err("a word nobody defined");
    assert_eq!(unknown.class, "invalid_input", "{unknown:?}");

    // Nothing to name the tree after is a refusal too, never the shared tree.
    let nowhere = SharedState::new();
    let nameless = action
        .execute(&json!({"bin": bin, "tree": "own", "timeout_secs": 30}), &nowhere)
        .expect_err("no run, no step, no project");
    assert_eq!(nameless.class, "invalid_input", "{nameless:?}");
}
