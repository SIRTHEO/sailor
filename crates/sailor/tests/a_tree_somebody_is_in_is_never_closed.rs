//! A tree a person is working in is kept, whoever asks for it to go.
//!
//! The sweep asked who was standing there; `sailor worktree close <name>` did
//! not ask at all, and a delivery flow asked a third way in shell. Fault 267.

use sailor::retire_index::IndexTending;
use sailor::worktree_cmd::close_one;
use std::path::{Path, PathBuf};
use std::process::Command;
use workspace::standing::{who_is_in, WhoIsIn};
use workspace::OpenTrees;

const A_TRUNK: &str = "tronco";

const NOBODY: &[PathBuf] = &[];
const NO_PROCESS: &[(u32, PathBuf)] = &[];

fn a_scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-standing-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch");
    path
}

fn run_git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

fn a_repository_in(scratch: &Path) -> PathBuf {
    let repo = scratch.join("project");
    std::fs::create_dir_all(&repo).expect("the repository");
    run_git(&repo, &["init", "-q"]);
    run_git(&repo, &["config", "user.email", "prove@example"]);
    run_git(&repo, &["config", "user.name", "prove"]);
    std::fs::write(repo.join("README"), "a tree to cut from\n").expect("a file");
    run_git(&repo, &["add", "README"]);
    run_git(&repo, &["commit", "-q", "-m", "the first"]);
    run_git(&repo, &["branch", "-M", A_TRUNK]);
    run_git(&repo, &["config", workspace::branches::TRUNK_KEY, A_TRUNK]);
    repo
}

/// The tree, the store and the tending a close needs, plus the scratch to sweep
/// away afterwards.
struct AClose {
    scratch: PathBuf,
    repo: PathBuf,
    tree: PathBuf,
    store: ledger::Ledger,
    tending: IndexTending,
}

fn a_tree_ready_to_close(label: &str) -> AClose {
    let scratch = a_scratch(label);
    let repo = a_repository_in(&scratch);
    let store = ledger::Ledger::open(scratch.join("store")).expect("a store");
    let tree = workspace::create(&repo, "work/done", None, &store).expect("a merged tree");
    let tending = IndexTending::of(&repo);
    AClose {
        scratch,
        repo,
        tree,
        store,
        tending,
    }
}

impl AClose {
    fn close(&self, occupied: &[PathBuf], standing: &[(u32, PathBuf)]) -> Result<String, String> {
        close_one(
            &self.repo,
            "done",
            &self.store as &dyn OpenTrees,
            &self.tending,
            occupied,
            standing,
        )
    }
}

#[test]
fn a_tree_nobody_is_in_is_closed() {
    let it = a_tree_ready_to_close("free");

    let said = it.close(NOBODY, NO_PROCESS);
    let gone = !it.tree.exists();
    let _ = std::fs::remove_dir_all(&it.scratch);

    assert!(said.is_ok(), "{said:?}");
    assert!(gone, "{said:?}");
}

#[test]
fn a_tree_with_a_terminal_recorded_in_it_is_kept() {
    let it = a_tree_ready_to_close("terminal");

    let why = it.close(std::slice::from_ref(&it.tree), NO_PROCESS);
    let still_there = it.tree.exists();
    let _ = std::fs::remove_dir_all(&it.scratch);

    let why = why.expect_err("a tree a terminal is in is kept");
    assert!(still_there, "{why}");
    assert!(
        why.contains(&it.tree.to_string_lossy().to_string()),
        "{why}"
    );
}

/// **A SHELL ONE DIRECTORY DOWN IS STILL A SHELL IN THE TREE.** The reading
/// compared a terminal against the tree itself and nothing else, so a session
/// working in a subdirectory read as nobody.
#[test]
fn a_terminal_one_directory_down_is_still_in_the_tree() {
    let it = a_tree_ready_to_close("below");
    let below = it.tree.join("crates");
    std::fs::create_dir_all(&below).expect("a directory inside");

    let why = it.close(&[below], NO_PROCESS);
    let still_there = it.tree.exists();
    let _ = std::fs::remove_dir_all(&it.scratch);

    assert!(why.is_err(), "{why:?}");
    assert!(still_there, "{why:?}");
}

#[test]
fn a_tree_a_process_stands_in_is_kept_and_the_process_is_named() {
    let it = a_tree_ready_to_close("process");

    let why = it.close(NOBODY, &[(4242, it.tree.join("crates"))]);
    let still_there = it.tree.exists();
    let _ = std::fs::remove_dir_all(&it.scratch);

    let why = why.expect_err("a tree a process stands in is kept");
    assert!(still_there, "{why}");
    assert!(why.contains("4242"), "{why}");
}

#[test]
fn a_terminal_in_another_tree_is_not_in_this_one() {
    let at = std::env::temp_dir();

    let found = who_is_in(
        &at.join("one"),
        &[at.join("another")],
        &[(7, at.join("elsewhere"))],
    );

    assert_eq!(found, WhoIsIn::Nobody);
}

/// A path that shares a prefix with the tree's name is not inside it: the
/// reading is by directory, not by the letters of a path.
#[test]
fn a_neighbour_whose_name_begins_the_same_is_not_inside() {
    let at = std::env::temp_dir();

    let found = who_is_in(&at.join("one"), &[at.join("one-more")], NO_PROCESS);

    assert_eq!(found, WhoIsIn::Nobody);
}
