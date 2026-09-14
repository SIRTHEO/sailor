//! A repository of the tests' own: a one-function crate with a bug, the fix
//! that adds the test proving it, a commit that touches only that test, and a
//! fix whose test was green before it. Built with git and cut down with the
//! test, so no test reads a repository of this machine.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct FixtureRepository {
    pub root: PathBuf,
    pub repo: PathBuf,
    pub home: PathBuf,
    pub target: PathBuf,
    pub bug: String,
    pub fix: String,
    pub tests_only: String,
    pub green_fix: String,
}

pub const FIX_BODY: &str = "The adder subtracted what it was asked to add.\nSee fault 7.";

impl FixtureRepository {
    pub fn new(label: &str) -> Self {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sailor-bench-{label}-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("the fixture's directory");
        git(&repo, &["init", "-q"]);
        write(
            &repo,
            "Cargo.toml",
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        );
        write(&repo, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n");
        write(&repo, ".gitignore", "target/\nCargo.lock\n");
        let bug = commit(&repo, "feat: the adder\n\nOne function, so a test has something to call.");

        write(&repo, "src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n");
        write(
            &repo,
            "tests/adds.rs",
            "#[test]\nfn two_and_two_make_four() {\n    assert_eq!(fixture::add(2, 2), 4);\n}\n",
        );
        let fix = commit(&repo, &format!("fix(adder): two and two make four\n\n{FIX_BODY}\n"));

        write(
            &repo,
            "tests/adds.rs",
            "// The one test of the adder.\n#[test]\nfn two_and_two_make_four() {\n    assert_eq!(fixture::add(2, 2), 4);\n}\n",
        );
        let tests_only = commit(&repo, "fix(tests): say what the file holds\n\nA comment only.");

        write(
            &repo,
            "src/lib.rs",
            "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
        );
        write(&repo, "tests/green.rs", "#[test]\nfn one_is_one() {\n    assert_eq!(1, 1);\n}\n");
        let green_fix = commit(
            &repo,
            "fix(docs): say what the adder is for\n\nA doc line, and a test that was green before it.",
        );

        Self {
            home: root.join("home"),
            target: root.join("target"),
            root,
            repo,
            bug,
            fix,
            tests_only,
            green_fix,
        }
    }
}

impl Drop for FixtureRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write(repo: &Path, relative: &str, text: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the file's directory");
    }
    std::fs::write(path, text).expect("the fixture file");
}

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
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["-c", "commit.gpgsign=false", "commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}
