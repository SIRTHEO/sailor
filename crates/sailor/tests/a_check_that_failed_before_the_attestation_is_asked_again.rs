//! `merge_request` of `integrate-on-the-trunk`, run as the flow runs it against a
//! forge that answers from a file. A check that waits for the private-names
//! attestation gave up while a person was still at the manual gates, and the
//! merge was refused on a commit that was attested by then (fault 270).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const THE_FLOW: &str = "crates/flow/system/integrate-on-the-trunk.flow.json";
const THE_STEP: &str = "merge_request";
const LONG_AGO: &str = "2000-01-01T00:00:00Z";
const LONG_AFTER: &str = "2999-01-01T00:00:00Z";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
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

struct Forge(PathBuf);

impl Forge {
    /// The checks the forge shows now, and the ones it shows once a run is asked again.
    fn new(label: &str, now: &str, asked_again: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sailor-asked-again-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).expect("a bin directory");
        std::fs::create_dir_all(root.join("repo")).expect("a repository directory");
        std::fs::write(root.join("checks.json"), now).expect("the checks");
        std::fs::write(root.join("checks-asked-again.json"), asked_again).expect("the checks asked again");
        let gh = root.join("bin").join("gh");
        std::fs::write(
            &gh,
            "#!/bin/sh\n\
             here=$(dirname \"$0\")/..\n\
             case \"$1 $2\" in\n\
             'auth token') echo \"token-for-$4\" ;;\n\
             'pr checks') while [ \"$1\" != --jq ]; do shift; done; jq -r \"$2\" \"$here/checks.json\" ;;\n\
             'run rerun') echo \"$*\" >> \"$here/calls\"; cp \"$here/checks-asked-again.json\" \"$here/checks.json\" ;;\n\
             *) echo \"$*\" >> \"$here/calls\" ;;\n\
             esac\n",
        )
        .expect("the forge");
        let chmod = Command::new("chmod").arg("+x").arg(&gh).status().expect("chmod runs");
        assert!(chmod.success());
        let init = Command::new("git").arg("-C").arg(root.join("repo")).args(["init", "-q"]).status().expect("git runs");
        assert!(init.success());
        let config = Command::new("git")
            .arg("-C")
            .arg(root.join("repo"))
            .args(["config", "sailor.forgeAs", "an-account"])
            .status()
            .expect("git runs");
        assert!(config.success());
        Forge(root)
    }

    fn merge(&self) -> Output {
        Command::new("sh")
            .arg("-c")
            .arg(the_command())
            .env("PATH", format!("{}:{}", self.0.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("REPO", self.0.join("repo"))
            .env("FORGE_REPO", "")
            .env("NUMBER", "7")
            .env("COMMIT", "abc123")
            .env("ALREADY", "false")
            .env("POLL_SECS", "0")
            .output()
            .expect("the step runs")
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.0.join("calls")).unwrap_or_default()
    }
}

impl Drop for Forge {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check(name: &str, bucket: &str, completed: &str, link: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "bucket": bucket, "completedAt": completed, "link": link })
}

fn checks(all: &[serde_json::Value]) -> String {
    serde_json::Value::Array(all.to_vec()).to_string()
}

const A_RUN: &str = "https://forge.example/an-owner/a-repository/actions/runs/111/job/222";
const A_STATUS: &str = "";

fn said(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn a_check_that_gave_up_before_the_attestation_is_asked_again_and_the_merge_goes_on() {
    let forge = Forge::new(
        "gave-up",
        &checks(&[check("boundary", "fail", LONG_AGO, A_RUN), check("names", "pass", LONG_AGO, A_STATUS)]),
        &checks(&[check("boundary", "pass", LONG_AFTER, A_RUN), check("names", "pass", LONG_AGO, A_STATUS)]),
    );
    let merged = forge.merge();
    workspace::measured(1, "merge_request command read from the shipped flow");
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(forge.calls(), "run rerun 111 --failed\npr merge 7 --merge\n");
}

#[test]
fn a_check_still_red_when_asked_again_refuses_the_merge_after_one_try() {
    let red = checks(&[check("boundary", "fail", LONG_AGO, A_RUN)]);
    let forge = Forge::new("still-red", &red, &red);
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert!(said(&refused).contains("not green on pull request 7: boundary"), "{}", said(&refused));
    assert_eq!(forge.calls(), "run rerun 111 --failed\n");
}

#[test]
fn a_check_that_failed_after_the_attestation_is_not_asked_again() {
    let red = checks(&[check("boundary", "fail", LONG_AFTER, A_RUN)]);
    let forge = Forge::new("after", &red, &red);
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert_eq!(forge.calls(), "");
}

#[test]
fn a_red_status_that_no_run_carries_refuses_without_asking_anything() {
    let red = checks(&[check("names", "fail", LONG_AGO, A_STATUS)]);
    let forge = Forge::new("status", &red, &red);
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert_eq!(forge.calls(), "");
}

#[test]
fn all_green_merges_without_asking_anything_again() {
    let green = checks(&[check("boundary", "pass", LONG_AGO, A_RUN)]);
    let forge = Forge::new("green", &green, &green);
    let merged = forge.merge();
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(forge.calls(), "pr merge 7 --merge\n");
}
