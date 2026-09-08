//! A release built from HEAD over an uncommitted tree ships work nobody sees.
//! The refusal is only worth its sentence if it lands before the clone, the
//! build and the install: this judge runs the real binary against a throwaway
//! repository and looks at what it left behind.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn scratch(label: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!(
        "sailor-dirty-release-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(&at).expect("the scratch directory is made");
    at
}

fn git(repo: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git starts");
    assert!(
        done.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&done.stderr)
    );
}

/// A repository whose committed HEAD is clean and whose working tree is not,
/// with the difference sitting inside the parts the `sailor` target is made of.
fn a_repository_with_work_left_out(at: &Path) -> PathBuf {
    let repo = at.join("sources");
    std::fs::create_dir_all(repo.join("crates/harbourmaster/src"))
        .expect("the sources directory is made");
    std::fs::write(repo.join("crates/harbourmaster/src/lib.rs"), "pub fn moored() {}\n")
        .expect("the committed file is written");
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["config", "user.email", "keeper@example.invalid"]);
    git(&repo, &["config", "user.name", "The Harbourmaster"]);
    git(&repo, &["add", "crates/harbourmaster/src/lib.rs"]);
    git(&repo, &["commit", "--quiet", "-m", "the first mooring"]);
    std::fs::write(
        repo.join("crates/harbourmaster/src/lib.rs"),
        "pub fn moored() { unimplemented!() }\n",
    )
    .expect("the uncommitted change is written");
    repo
}

fn release_in(repo: &Path, home: &Path, extra: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sailor"));
    command
        .current_dir(repo)
        .args(["release", "sailor"])
        .args(extra)
        .env("SAILOR_SOURCES", repo)
        .env("SAILOR_HOME", home);
    command.output().expect("the binary starts")
}

fn spoken(said: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&said.stdout),
        String::from_utf8_lossy(&said.stderr)
    )
}

#[test]
fn a_release_over_uncommitted_work_leaves_no_clone_no_build_and_no_binary() {
    let at = scratch("refused");
    let repo = a_repository_with_work_left_out(&at);
    let home = at.join("home");
    std::fs::create_dir_all(&home).expect("the house is made");

    let said = release_in(&repo, &home, &[]);
    let spoke = spoken(&said);

    assert!(!said.status.success(), "the release went through: {spoke}");
    assert!(
        spoke.contains("would stay out of service"),
        "it stopped for some other reason: {spoke}"
    );
    assert!(
        !repo.join("target").exists(),
        "the clone or the build had already started: {:?}",
        repo.join("target")
    );
    assert!(
        !home.join("bin").exists(),
        "something was installed before the refusal: {:?}",
        home.join("bin")
    );

    let _ = std::fs::remove_dir_all(&at);
}

#[test]
fn the_word_said_outright_carries_the_release_past_the_uncommitted_work() {
    let at = scratch("allowed");
    let repo = a_repository_with_work_left_out(&at);
    let home = at.join("home");
    std::fs::create_dir_all(&home).expect("the house is made");

    let said = release_in(&repo, &home, &["--even-if-dirty"]);
    let spoke = spoken(&said);

    // It still fails — there is nothing in this repository to build — but it
    // fails past the gate, and the gate is what is under test.
    assert!(
        repo.join("target").exists(),
        "the gate stopped it anyway, with the word said outright: {spoke}"
    );

    let _ = std::fs::remove_dir_all(&at);
}
