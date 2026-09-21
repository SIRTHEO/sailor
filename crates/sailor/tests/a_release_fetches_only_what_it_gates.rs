//! The candidate step of `cut-a-release` reads the remote trunk and the version
//! tags, and nothing else. It used to fetch every tag in silence, so an archive
//! tag rewritten on the remote stopped the release with an empty reason.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const THE_FLOW: &str = "crates/flow/system/cut-a-release.flow.json";
const THE_STEP: &str = "the_candidate";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::var_os("SAILOR_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = root.join(format!(
            "sailor-release-fetch-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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
        .expect("the candidate step carries a command")
        .to_owned()
}

fn git(at: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "A Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.test")
        .env("GIT_COMMITTER_NAME", "A Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.test")
        .output()
        .expect("git runs")
}

fn ok(at: &Path, args: &[&str]) -> String {
    let out = git(at, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A remote with a trunk, and a clone of it whose `tag` names another commit
/// than the remote's.
fn a_clone_whose_tag_disagrees(scratch: &Scratch, tag: &str) -> PathBuf {
    let remote = scratch.0.join("remote.git");
    let local = scratch.0.join("local");
    ok(
        &scratch.0,
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    ok(
        &scratch.0,
        &[
            "init",
            "--quiet",
            "--initial-branch=main",
            local.to_str().unwrap(),
        ],
    );
    ok(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    ok(
        &local,
        &["commit", "--quiet", "--allow-empty", "-m", "first"],
    );
    let first = ok(&local, &["rev-parse", "HEAD"]);
    ok(
        &local,
        &["commit", "--quiet", "--allow-empty", "-m", "second"],
    );
    ok(&local, &["push", "--quiet", "origin", "main"]);
    ok(
        &local,
        &[
            "push",
            "--quiet",
            "origin",
            &format!("HEAD:refs/tags/{tag}"),
        ],
    );
    ok(&local, &["tag", "--force", tag, &first]);
    local
}

fn run_the_step(repo: &Path) -> Output {
    Command::new("sh")
        .arg("-c")
        .arg(the_command())
        .env("REPO", repo)
        .env("BASE", "main")
        .env("VERSION", "0.1.0")
        .env("REMOTE_NAME", "origin")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("sh runs")
}

#[test]
fn an_archive_tag_the_remote_rewrote_does_not_stop_a_release() {
    let scratch = Scratch::new("archive");
    let local = a_clone_whose_tag_disagrees(&scratch, "archive/work/a-branch");
    let out = run_the_step(&local);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let said: serde_json::Value = serde_json::from_slice(&out.stdout).expect("one JSON answer");
    assert_eq!(said["tag"], "v0.1.0");
    assert_eq!(
        said["candidate"].as_str(),
        Some(ok(&local, &["rev-parse", "main"]).as_str())
    );
}

#[test]
fn a_version_tag_that_disagrees_stops_the_release_and_says_why() {
    let scratch = Scratch::new("version");
    let local = a_clone_whose_tag_disagrees(&scratch, "v0.0.9");
    let out = run_the_step(&local);
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("was refused") && said.contains("v0.0.9"),
        "{said}"
    );
}
