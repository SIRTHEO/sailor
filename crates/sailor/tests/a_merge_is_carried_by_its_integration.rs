//! `integrated` of `integrate-on-the-trunk`, run as the flow runs it against a
//! forge that writes its calls down. The trunk requires `sailor/integrated`,
//! and this step is the only thing that posts it: on the reviewed head, as the
//! tree's account, naming the run and the commit the verdict was given on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const THE_FLOW: &str = "crates/flow/system/integrate-on-the-trunk.flow.json";
const THE_STEP: &str = "integrated";
const THE_SCRIPT: &str = "scripts/attest-integration.sh";
const A_RUN: &str = "integrate-on-the-trunk-42";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

fn the_command() -> String {
    let text = std::fs::read_to_string(workspace_root().join(THE_FLOW)).expect("the flow file");
    let flow: serde_json::Value = serde_json::from_str(&text).expect("the flow parses");
    flow["graph"]["steps"]
        .as_array()
        .expect("the steps")
        .iter()
        .find(|step| step["id"] == THE_STEP)
        .and_then(|step| step["with"]["command"].as_str())
        .expect("the step carries a command")
        .to_owned()
}

struct Scratch(PathBuf);

impl Scratch {
    /// A repository with one commit, the script beside it, and a `gh` that
    /// answers a token per account and writes down every other call. Asked
    /// to refuse, it refuses every call that is not for a token.
    fn new(label: &str, forge_refuses: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sailor-integrated-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join("scripts")).expect("a scripts directory");
        std::fs::create_dir_all(root.join("bin")).expect("a bin directory");
        std::fs::copy(workspace_root().join(THE_SCRIPT), repo.join(THE_SCRIPT))
            .expect("the script is in the tree");
        let refusal = if forge_refuses { "exit 1\n" } else { "" };
        let gh = root.join("bin").join("gh");
        std::fs::write(
            &gh,
            format!(
                "#!/bin/sh\nif [ \"$1\" = auth ]; then echo \"token-for-$4\"; exit 0; fi\n{refusal}\
                 printf '%s\\n' \"GH_TOKEN=$GH_TOKEN $*\" >> \"$GH_CALLS\"\n"
            ),
        )
        .expect("the recording forge");
        let scratch = Scratch(root);
        scratch.run("chmod", &["+x", gh.to_str().expect("utf-8")]);
        scratch.git(&["-c", "init.defaultBranch=main", "init", "-q"]);
        scratch.git(&["config", "user.email", "test@example.test"]);
        scratch.git(&["config", "user.name", "test"]);
        scratch.git(&["config", "sailor.forgeAs", "an-account"]);
        scratch.git(&["remote", "add", "origin", "https://github.com/an-owner/a-repository.git"]);
        std::fs::write(repo.join("readme.txt"), "a project\n").expect("a file");
        scratch.git(&["add", "readme.txt"]);
        scratch.git(&["commit", "-q", "-m", "the reviewed head"]);
        scratch
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn run(&self, program: &str, args: &[&str]) -> String {
        let done = Command::new(program)
            .args(args)
            .current_dir(self.repo())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("the program runs");
        assert!(done.status.success(), "{program} {args:?}: {}", said(&done));
        String::from_utf8_lossy(&done.stdout).trim().to_owned()
    }

    fn git(&self, args: &[&str]) -> String {
        self.run("git", args)
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    fn the_step(&self, commit: &str, run: Option<&str>) -> Output {
        let mut step = Command::new("sh");
        step.arg("-c")
            .arg(the_command())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.0.join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GH_CALLS", self.0.join("calls"))
            .env("REPO", self.repo())
            .env("COMMIT", commit)
            .env_remove("SAILOR_RUN");
        if let Some(run) = run {
            step.env("SAILOR_RUN", run);
        }
        step.output().expect("the step runs")
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.0.join("calls")).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn the_status_names_the_run_and_the_reviewed_head_as_the_tree_s_account() {
    let scratch = Scratch::new("posted", false);
    let head = scratch.head();
    workspace::measured(1, "integrated command read from the shipped flow");

    let posted = scratch.the_step(&head, Some(A_RUN));
    assert!(posted.status.success(), "{}", said(&posted));
    let answer: serde_json::Value =
        serde_json::from_slice(&posted.stdout).expect("the step answers in JSON");
    assert_eq!(answer, serde_json::json!({"on": head, "run": A_RUN}));

    let calls = scratch.calls();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.starts_with("GH_TOKEN=token-for-an-account "), "not the tree's account: {calls}");
    assert!(
        calls.contains(&format!("repos/an-owner/a-repository/statuses/{head} ")),
        "not the reviewed head: {calls}"
    );
    assert!(calls.contains("state=success") && calls.contains("context=sailor/integrated"), "{calls}");
    assert!(
        calls.contains(&format!("description=run {A_RUN}; verdict on {}", &head[..12])),
        "the status does not name the run and the verdict's commit: {calls}"
    );
}

#[test]
fn a_step_told_no_run_posts_nothing() {
    let scratch = Scratch::new("no-run", false);
    let refused = scratch.the_step(&scratch.head(), None);
    assert_ne!(refused.status.code(), Some(0), "{}", said(&refused));
    assert!(said(&refused).contains("no run names this integration"), "{}", said(&refused));
    assert_eq!(scratch.calls(), "", "a status was posted with no run to name");
}

#[test]
fn a_status_the_forge_refuses_breaks_the_step_and_says_why() {
    let scratch = Scratch::new("refused", true);
    let refused = scratch.the_step(&scratch.head(), Some(A_RUN));
    assert_ne!(refused.status.code(), Some(0), "{}", said(&refused));
    assert!(
        said(&refused).contains("the forge refused the status, so nothing is merged"),
        "{}",
        said(&refused)
    );
    assert!(refused.stdout.is_empty(), "a refused status still answered: {}", said(&refused));
}
