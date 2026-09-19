//! A branch is held to the convention where it is born, not where it is counted.
//!
//! `sailor worktree names` reports the names already standing, which is a
//! reading and not a refusal: every stray branch this repository carries got
//! its name past that report. The refusal belongs at the one gesture that
//! makes a branch.

use std::path::{Path, PathBuf};
use std::process::Command;
use workspace::branches::{may_be_cut, THE_CONVENTION};

const THE_BRANCH_EXISTS: bool = true;
const A_NEW_BRANCH: bool = false;

#[test]
fn a_new_branch_named_for_its_work_is_cut() {
    assert!(may_be_cut("work/the-trees-are-swept", A_NEW_BRANCH).is_ok());
}

#[test]
fn a_new_branch_against_the_convention_is_refused() {
    for name in ["t", "tmp/rebuild-68", "fix-docs", "crew/round1-a4"] {
        assert!(
            may_be_cut(name, A_NEW_BRANCH).is_err(),
            "«{name}» was cut anyway"
        );
    }
}

/// A refusal nobody can act on sends the reader back to the source.
#[test]
fn the_refusal_names_the_branch_and_says_the_shape() {
    let refusal = may_be_cut("fix-docs", A_NEW_BRANCH).unwrap_err();
    assert!(refusal.contains("fix-docs"), "{refusal}");
    assert!(refusal.contains(THE_CONVENTION), "{refusal}");
}

/// **A NAME ALREADY GIVEN IS NOT THIS CHECK'S BUSINESS.** Refusing it would
/// lock every tree out of the branches standing today, and out of the ones a
/// forge or another person named.
#[test]
fn a_branch_that_already_exists_keeps_the_name_it_was_given() {
    assert!(may_be_cut("fix-docs", THE_BRANCH_EXISTS).is_ok());
    assert!(may_be_cut("matteodimattia/window-three", THE_BRANCH_EXISTS).is_ok());
}

#[test]
fn the_trunk_is_cut_like_any_branch_that_follows_the_convention() {
    assert!(may_be_cut(workspace::branches::TRUNK, A_NEW_BRANCH).is_ok());
}

/// **THE JUDGE ABOVE IS ONLY HALF OF IT.** A refusal no gesture asks for is a
/// rule the repository does not have: this cuts trees for real.
#[test]
fn the_gesture_that_cuts_a_tree_asks() {
    let scratch = a_scratch("cutting");
    let repo = a_repository_in(&scratch);

    let refused = workspace::create(&repo, "fix-docs", None);
    let left_behind = workspace::tree_path(&repo, "fix-docs").exists();
    let cut = workspace::create(&repo, "work/a-topic-of-its-own", None);

    run_git(&repo, &["branch", "fix-docs"]);
    let already_named = workspace::create(&repo, "fix-docs", None);

    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(repo.with_file_name("project-worktrees"));

    assert!(refused.is_err(), "a stray name was cut anyway");
    assert!(!left_behind, "the refusal still left a tree behind");
    assert!(cut.is_ok(), "a named branch was refused: {cut:?}");
    assert!(
        already_named.is_ok(),
        "a branch already standing was locked out of a tree: {already_named:?}"
    );
}

fn a_scratch(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("sailor-cut-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch");
    path
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
    run_git(&repo, &["branch", "-M", workspace::branches::TRUNK]);
    repo
}

/// Only the repository's own settings: nothing of the account running this.
fn run_git(at: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// **THE REFUSAL NAMED A TRUNK THAT NO LONGER EXISTS.** It said «sorgenti»
/// for however long after the rename, sending every reader to a branch git
/// would not find; the name now comes from the one place that holds it.
#[test]
fn the_verdict_on_the_names_says_the_trunk_this_repository_has() {
    let said = sailor::worktree_cmd::names(&[String::from("fix-docs")])
        .expect_err("a stray name is bad news, and bad news is an error");
    assert!(
        said.contains(workspace::branches::TRUNK),
        "the verdict names no trunk: {said}"
    );
    assert!(
        !said.contains("sorgenti"),
        "the verdict still names the old trunk: {said}"
    );
}
