//! The gate that runs before a push, a pull request, an integration, a tag and
//! a release, asked with the four cases a boundary must refuse: no
//! configuration, a private name, a home path, and a workshop passage that
//! carries neither.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const INVENTED_HOME_OF_ANOTHER_MACHINE: &str = "/Users/a-name-nobody-here-has";
const SYNTHETIC_PRIVATE_NAME: &str = "mylberry";

fn scripts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts")
}

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sailor-boundary-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(root.join("home")).expect("a home for the scratch");
        std::fs::create_dir_all(root.join("repo")).expect("a repository directory");
        Scratch { root }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn list(&self) -> PathBuf {
        self.root.join("declared-private-names")
    }

    fn declare_names(&self) {
        std::fs::write(self.list(), format!("# synthetic\n{SYNTHETIC_PRIVATE_NAME}\n"))
            .expect("the declared list");
    }

    fn git(&self, args: &[&str]) {
        let done = Command::new("git")
            .arg("-C")
            .arg(self.repo())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    }

    /// `main` with one commit, and `work/change` one commit ahead of it.
    fn a_branch_ahead(&self, file_text: &str, message: &str) {
        self.git(&["-c", "init.defaultBranch=main", "init", "-q"]);
        self.git(&["config", "user.email", "test@example.test"]);
        self.git(&["config", "user.name", "test"]);
        self.git(&["config", "sailor.forgeAs", "an-account"]);
        self.git(&["remote", "add", "origin", "https://github.com/an-owner/a-repository.git"]);
        std::fs::write(self.repo().join("readme.txt"), "a project\n").expect("first file");
        self.git(&["add", "readme.txt"]);
        self.git(&["commit", "-q", "-m", "the first"]);
        self.git(&["checkout", "-q", "-b", "work/change"]);
        std::fs::write(self.repo().join("change.txt"), file_text).expect("changed file");
        self.git(&["add", "change.txt"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    fn run(&self, script: &str, args: &[&str]) -> Output {
        Command::new(scripts().join(script))
            .args(args)
            .current_dir(self.repo())
            .env("HOME", self.home())
            .env("SAILOR_PRIVATE_NAMES", self.list())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env("GH_CALLS", self.root.join("gh-calls"))
            .output()
            .expect("the script starts")
    }

    /// A `gh` that answers a token per account and writes down every other call.
    fn a_recording_forge(&self) {
        let bin = self.root.join("bin");
        std::fs::create_dir_all(&bin).expect("a bin directory");
        let gh = bin.join("gh");
        std::fs::write(
            &gh,
            "#!/bin/sh\nif [ \"$1\" = auth ]; then echo \"token-for-$4\"; exit 0; fi\nprintf '%s\\n' \"GH_TOKEN=$GH_TOKEN $*\" >> \"$GH_CALLS\"\n",
        )
        .expect("the recording forge");
        let chmod = Command::new("chmod").arg("+x").arg(&gh).status().expect("chmod runs");
        assert!(chmod.success());
    }

    fn forge_calls(&self) -> String {
        std::fs::read_to_string(self.root.join("gh-calls")).unwrap_or_default()
    }

    fn text(&self, words: &str) -> PathBuf {
        let at = self.root.join("outgoing.md");
        std::fs::write(&at, words).expect("the outgoing text");
        at
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn said(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn an_absent_configuration_is_never_a_pass() {
    let scratch = Scratch::new("absent");
    scratch.a_branch_ahead("an ordinary change\n", "an ordinary change");
    scratch.a_recording_forge();

    let branch = scratch.run("privacy-scan.sh", &["work/change"]);
    assert_eq!(branch.status.code(), Some(2), "{}", said(&branch));

    let outgoing = scratch.text("An ordinary pull request body.\n");
    let text = scratch.run("privacy-scan.sh", &["--text", outgoing.to_str().expect("utf-8")]);
    assert_eq!(text.status.code(), Some(2), "{}", said(&text));

    let attested = scratch.run("attest-private-names.sh", &["work/change"]);
    assert_ne!(attested.status.code(), Some(0), "{}", said(&attested));
    assert_eq!(scratch.forge_calls(), "", "a status was posted for a check that never ran");
}

#[test]
fn a_synthetic_private_name_is_refused_and_not_echoed() {
    let scratch = Scratch::new("name");
    scratch.declare_names();
    scratch.a_branch_ahead(&format!("a line about {SYNTHETIC_PRIVATE_NAME}\n"), "an ordinary change");

    let branch = scratch.run("privacy-scan.sh", &["work/change"]);
    assert_eq!(branch.status.code(), Some(1), "{}", said(&branch));
    assert!(!said(&branch).contains(SYNTHETIC_PRIVATE_NAME), "the refusal echoed the name");

    let outgoing = scratch.text(&format!("Thanks to {SYNTHETIC_PRIVATE_NAME} for the report.\n"));
    let text = scratch.run("privacy-scan.sh", &["--text", outgoing.to_str().expect("utf-8")]);
    assert_eq!(text.status.code(), Some(1), "{}", said(&text));
    assert!(!said(&text).contains(SYNTHETIC_PRIVATE_NAME), "the refusal echoed the name");
}

#[test]
fn a_home_path_of_any_machine_is_refused() {
    let scratch = Scratch::new("home");
    scratch.declare_names();
    scratch.a_branch_ahead(
        "an ordinary change\n",
        &format!("fix: measured in {INVENTED_HOME_OF_ANOTHER_MACHINE}/personal/project"),
    );

    let branch = scratch.run("privacy-scan.sh", &["work/change"]);
    assert_eq!(branch.status.code(), Some(1), "{}", said(&branch));
}

#[test]
fn a_workshop_passage_without_tokens_is_refused_in_a_public_text() {
    let scratch = Scratch::new("workshop");
    scratch.declare_names();
    scratch.a_branch_ahead("an ordinary change\n", "an ordinary change");

    for passage in [
        "Measured on this machine, the second round stopped when its budget ran out.\n",
        "The orphan kept pid 91964 alive after the window closed.\n",
        "The relay answered on 127.0.0.1:4317 while the session was open.\n",
        "Seen in session 2c886544-0f1e-4c1a-9d2b-3e4f5a6b7c8d on ttys008.\n",
    ] {
        let outgoing = scratch.text(passage);
        let text = scratch.run("privacy-scan.sh", &["--text", outgoing.to_str().expect("utf-8")]);
        assert_eq!(text.status.code(), Some(1), "«{passage}» passed: {}", said(&text));
    }
}

#[test]
fn a_clean_branch_and_a_clean_text_pass() {
    let scratch = Scratch::new("clean");
    scratch.declare_names();
    scratch.a_branch_ahead("an ordinary change\n", "fix(flow): a step reads its own input");

    let branch = scratch.run("privacy-scan.sh", &["work/change"]);
    assert_eq!(branch.status.code(), Some(0), "{}", said(&branch));

    let outgoing = scratch.text("## Problem\nA step read the wrong input.\n\n## Behaviour\nIt reads its own.\n");
    let text = scratch.run("privacy-scan.sh", &["--text", outgoing.to_str().expect("utf-8")]);
    assert_eq!(text.status.code(), Some(0), "{}", said(&text));
}

#[test]
fn a_clean_branch_is_attested_on_its_exact_commit_as_the_tree_s_account() {
    let scratch = Scratch::new("attest");
    scratch.declare_names();
    scratch.a_branch_ahead("an ordinary change\n", "fix(flow): a step reads its own input");
    scratch.a_recording_forge();
    let head = Command::new("git")
        .arg("-C")
        .arg(scratch.repo())
        .args(["rev-parse", "work/change"])
        .output()
        .expect("git runs");
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();

    let attested = scratch.run("attest-private-names.sh", &["work/change"]);
    assert_eq!(attested.status.code(), Some(0), "{}", said(&attested));

    let calls = scratch.forge_calls();
    assert!(calls.contains("GH_TOKEN=token-for-an-account "), "not the tree's account: {calls}");
    assert!(calls.contains(&format!("repos/an-owner/a-repository/statuses/{head}")), "not the exact commit: {calls}");
    assert!(calls.contains("state=success") && calls.contains("context=sailor/private-names"), "{calls}");
}

#[test]
fn without_the_list_the_listless_half_measures_shapes_and_says_names_were_not_measured() {
    let scratch = Scratch::new("listless");
    scratch.a_branch_ahead("an ordinary change\n", "fix(flow): a step reads its own input");

    let clean = scratch.run("privacy-scan.sh", &["--shapes-only", "work/change"]);
    assert_eq!(clean.status.code(), Some(0), "{}", said(&clean));
    assert!(said(&clean).contains("names: not measured"), "{}", said(&clean));

    let leaking = Scratch::new("listless-leak");
    leaking.a_branch_ahead("an ordinary change\n", "fix: the orphan kept pid 91964 alive");
    let refused = leaking.run("privacy-scan.sh", &["--shapes-only", "work/change"]);
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
}
