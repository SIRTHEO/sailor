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
    let path = dir.join("dove");
    fs::write(&path, "#!/bin/sh\npwd\n").expect("write the engine");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
    path.to_string_lossy().into_owned()
}

fn stood_in(outcome: &ActionOutcome) -> String {
    let ActionOutcome::Went(value) = outcome else {
        panic!("the step had to go: {outcome:?}");
    };
    value
        .get("stdout")
        .and_then(serde_json::Value::as_str)
        .expect("what the engine printed")
        .trim()
        .to_owned()
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
    let action = actions::ExternalEngineAction::new();
    let asks_for_a_tree = json!({"bin": bin, "tree": "own", "timeout_secs": 30});

    // The control: a step that asks for nothing stands where the run stands.
    let mut shared = shared_for(&repo, "corsa-1", "engine_a");
    let shared_tree = stood_in(
        &action
            .execute(&json!({"bin": bin, "timeout_secs": 30}), &mut shared)
            .expect("the step had to go"),
    );
    assert!(
        !shared_tree.contains("corsa-1"),
        "a step that asked for nothing was moved anyway: {shared_tree}"
    );

    let mut shared = shared_for(&repo, "corsa-1", "engine_a");
    let first = stood_in(&action.execute(&asks_for_a_tree, &mut shared).expect("the step had to go"));
    let mut shared = shared_for(&repo, "corsa-1", "engine_b");
    let second = stood_in(&action.execute(&asks_for_a_tree, &mut shared).expect("the step had to go"));

    assert_ne!(first, second, "two steps of one run were given the same tree");
    for stood in [&first, &second] {
        assert!(stood.contains("corsa-1"), "the tree is named after the run: {stood}");
        assert!(
            Path::new(stood).join("README").exists(),
            "the tree is a checkout of the project and not an empty directory: {stood}"
        );
    }
    assert!(first.ends_with("engine_a") && second.ends_with("engine_b"), "{first} / {second}");

    // A retried step finds the tree its first attempt left, or the work of the
    // attempt before it would be invisible to the one after.
    let mut shared = shared_for(&repo, "corsa-1", "engine_a");
    let again = stood_in(&action.execute(&asks_for_a_tree, &mut shared).expect("the step had to go"));
    assert_eq!(again, first, "a second attempt was cut a second tree");

    let listed = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "list"])
        .output()
        .expect("git lists what it cut");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains(&first) && listed.contains(&second),
        "git does not know the two trees, so they are directories and not checkouts:\n{listed}"
    );
}

#[test]
fn a_step_that_wants_a_tree_and_names_a_workdir_is_refused() {
    let dir = TempDir::new();
    let repo = a_repository_in(dir.path());
    let bin = an_engine_that_prints_where_it_stands(dir.path());
    let action = actions::ExternalEngineAction::new();

    let mut shared = shared_for(&repo, "corsa-2", "confuso");
    let refused = action
        .execute(
            &json!({"bin": bin, "tree": "own", "workdir": "/altrove", "timeout_secs": 30}),
            &mut shared,
        )
        .expect_err("two places for one step");
    assert_eq!(refused.class, "invalid_input", "{refused:?}");

    let mut shared = shared_for(&repo, "corsa-2", "sconosciuto");
    let unknown = action
        .execute(&json!({"bin": bin, "tree": "shared", "timeout_secs": 30}), &mut shared)
        .expect_err("a word nobody defined");
    assert_eq!(unknown.class, "invalid_input", "{unknown:?}");

    // Nothing to name the tree after is a refusal too, never the shared tree.
    let mut nowhere = SharedState::new();
    let nameless = action
        .execute(&json!({"bin": bin, "tree": "own", "timeout_secs": 30}), &mut nowhere)
        .expect_err("no run, no step, no project");
    assert_eq!(nameless.class, "invalid_input", "{nameless:?}");
}
