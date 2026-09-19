//! `sailor policy` reads `.sailor/delivery-policy.json` as committed on the
//! trunk the repository declares, never from the working tree and never from a
//! branch under review — a proposed change must never authorize itself. Which
//! branch that is, the repository says in `sailor.trunk`; the product assumes
//! no name of its own (ADR-020).

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sailor-policy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(dir: &Path) {
    init_repo_on(dir, "main");
}

/// The trunk is a name this repository chooses and declares. `main` is what
/// most of these repositories happen to call it, not something Sailor knows.
fn init_repo_on(dir: &Path, trunk: &str) {
    std::fs::create_dir_all(dir).expect("the repo directory");
    git(dir, &["init", "-q", "-b", trunk]);
    git(dir, &["config", "sailor.trunk", trunk]);
    git(dir, &["config", "user.email", "policy-test@example"]);
    git(dir, &["config", "user.name", "policy-test"]);
    std::fs::write(dir.join("readme"), "the repository this test builds\n")
        .expect("a first file");
    git(dir, &["add", "readme"]);
    git(dir, &["commit", "-q", "-m", "first"]);
}

fn commit_policy(dir: &Path, contents: &str) {
    std::fs::create_dir_all(dir.join(".sailor")).expect(".sailor directory");
    std::fs::write(dir.join(".sailor/delivery-policy.json"), contents).expect("write the policy");
    git(dir, &["add", ".sailor/delivery-policy.json"]);
    git(dir, &["commit", "-q", "-m", "policy"]);
}

fn run_policy(dir: &Path) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .arg("policy")
        .current_dir(dir)
        .output()
        .expect("the built binary runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A valid policy committed on `main`: the three settings, in order, and the
/// commit they were read from.
#[test]
fn a_valid_policy_on_the_trunk_prints_the_three_settings_and_the_commit() {
    let dir = scratch("valid");
    init_repo(&dir);
    commit_policy(
        &dir,
        r#"{"schema_version": 1, "merge": "auto", "push": "auto", "release": "ask"}"#,
    );
    let commit = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("utf8")
    .trim()
    .to_owned();

    let (ok, stdout, stderr) = run_policy(&dir);
    assert!(ok, "stdout={stdout} stderr={stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "merge: auto",
            "push: auto",
            "release: ask",
            "forge: not declared",
            "remote: not declared",
            commit.as_str(),
        ],
        "stdout={stdout}"
    );
}

/// **THE REGRESSION THIS FILE EXISTS FOR.** The trunk's name used to be a
/// string literal in the command, so a repository that calls its trunk
/// anything else refused its own policy and said only that there was none.
#[test]
fn a_repository_whose_trunk_is_not_called_main_reads_its_own_policy() {
    let dir = scratch("other-trunk");
    init_repo_on(&dir, "trunk");
    commit_policy(
        &dir,
        r#"{"schema_version": 1, "merge": "ask", "push": "auto", "release": "ask",
            "forge": "a-forge", "remote": "a-remote"}"#,
    );

    let (ok, stdout, stderr) = run_policy(&dir);
    assert!(ok, "stdout={stdout} stderr={stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines[..5],
        [
            "merge: ask",
            "push: auto",
            "release: ask",
            "forge: a-forge",
            "remote: a-remote"
        ],
        "stdout={stdout}"
    );
}

/// A repository that has declared nothing is told which declaration is
/// missing, rather than being refused with no reason given.
#[test]
fn a_repository_that_declares_no_trunk_is_told_which_declaration_is_missing() {
    let dir = scratch("no-trunk-declared");
    init_repo_on(&dir, "main");
    git(&dir, &["config", "--unset", "sailor.trunk"]);
    commit_policy(
        &dir,
        r#"{"schema_version": 1, "merge": "auto", "push": "auto", "release": "auto"}"#,
    );

    let (ok, stdout, stderr) = run_policy(&dir);
    assert!(!ok, "an undeclared trunk must refuse: stdout={stdout}");
    assert!(stdout.is_empty(), "nothing is printed to stdout: {stdout}");
    assert!(
        stderr.contains("sailor.trunk"),
        "the refusal names the declaration that is missing: {stderr}"
    );
}

/// The policy exists only on a side branch, never merged into `main`: the
/// working tree and the branch under review are exactly what this command
/// must not trust.
#[test]
fn a_policy_only_on_a_side_branch_is_no_policy_at_all() {
    let dir = scratch("side-branch");
    init_repo(&dir);
    git(&dir, &["checkout", "-q", "-b", "a-side-branch"]);
    commit_policy(
        &dir,
        r#"{"schema_version": 1, "merge": "auto", "push": "auto", "release": "auto"}"#,
    );
    git(&dir, &["checkout", "-q", "main"]);

    let (ok, stdout, stderr) = run_policy(&dir);
    assert!(!ok, "a policy absent from main must refuse: stdout={stdout}");
    assert!(stdout.is_empty(), "nothing is printed to stdout: {stdout}");
    assert!(
        stderr.contains("no policy on the trunk"),
        "the refusal names why: {stderr}"
    );
}

/// A field the schema does not allow: the refusal names which one.
#[test]
fn an_invalid_field_is_refused_by_name() {
    let dir = scratch("invalid-field");
    init_repo(&dir);
    commit_policy(
        &dir,
        r#"{"schema_version": 1, "merge": "maybe", "push": "auto", "release": "auto"}"#,
    );

    let (ok, stdout, stderr) = run_policy(&dir);
    assert!(!ok, "an invalid value must refuse: stdout={stdout}");
    assert!(stdout.is_empty(), "nothing is printed to stdout: {stdout}");
    assert!(stderr.contains("merge"), "the refusal names the field: {stderr}");
    assert!(
        stderr.contains("maybe"),
        "the refusal names what was there instead: {stderr}"
    );
}
