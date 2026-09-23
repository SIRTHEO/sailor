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
/// When attest posted, as its answer says: later than the clock of whoever runs this.
const ATTESTED_AT: &str = "2500-01-01T00:00:00Z";
const AFTER_THE_CLOCK_BEFORE_THE_ATTESTATION: &str = "2400-01-01T00:00:00Z";
/// The polls one hour holds, thirty seconds apart.
const AN_HOUR_OF_POLLS: usize = 120;

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

struct Forge(PathBuf);

/// One state of the forge: what `pr checks` shows, and, when it moves on by
/// itself, after how many polls for pending checks and to which state.
struct State<'a> {
    name: &'a str,
    checks: String,
    moves_on: Option<(usize, &'a str)>,
}

fn state<'a>(name: &'a str, checks: String) -> State<'a> {
    State {
        name,
        checks,
        moves_on: None,
    }
}

impl Forge {
    /// A forge that starts in the first state and goes to `asked_again` when a run is asked again.
    fn new(label: &str, states: &[State], asked_again: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("sailor-asked-again-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).expect("a bin directory");
        std::fs::create_dir_all(root.join("repo")).expect("a repository directory");
        for one in states {
            std::fs::write(root.join(format!("{}.json", one.name)), &one.checks).expect("a state");
            if let Some((polls, next)) = one.moves_on {
                std::fs::write(
                    root.join(format!("{}.after", one.name)),
                    format!("{polls} {next}\n"),
                )
                .expect("a move");
            }
        }
        std::fs::write(root.join("state"), states[0].name).expect("the first state");
        std::fs::write(root.join("asked-again"), asked_again).expect("the state asked again");
        let gh = root.join("bin").join("gh");
        std::fs::write(
            &gh,
            r#"#!/bin/sh
here=$(dirname "$0")/..
now=$(cat "$here/state")
case "$1 $2" in
'auth token') echo "token-for-$4" ;;
'pr checks')
    while [ $# -gt 0 ]; do
        case "$1" in --json) fields=$2 ;; --jq) filter=$2 ;; esac
        shift
    done
    jq -r "$filter" "$here/$now.json"
    if [ "$fields" = bucket ] && [ -f "$here/$now.after" ]; then
        read -r polls next < "$here/$now.after"
        served=$(( $(cat "$here/$now.served" 2>/dev/null || echo 0) + 1 ))
        echo "$served" > "$here/$now.served"
        [ "$served" -lt "$polls" ] || echo "$next" > "$here/state"
    fi ;;
'run rerun') echo "$*" >> "$here/calls"; cp "$here/asked-again" "$here/state" ;;
'pr merge')
    if jq -e 'any(.[]; .bucket == "pending")' "$here/$now.json" > /dev/null; then echo "merge while pending" >> "$here/calls"; exit 1; fi
    echo "$*" >> "$here/calls" ;;
*) echo "$*" >> "$here/calls" ;;
esac
"#,
        )
        .expect("the forge");
        let chmod = Command::new("chmod")
            .arg("+x")
            .arg(&gh)
            .status()
            .expect("chmod runs");
        assert!(chmod.success());
        let init = Command::new("git")
            .arg("-C")
            .arg(root.join("repo"))
            .args(["init", "-q"])
            .status()
            .expect("git runs");
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
        self.merge_posted_on("abc123")
    }

    fn merge_posted_on(&self, posted_on: &str) -> Output {
        Command::new("sh")
            .arg("-c")
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
            .env("REPO", self.0.join("repo"))
            .env("FORGE_REPO", "")
            .env("NUMBER", "7")
            .env("COMMIT", "abc123")
            .env("ALREADY", "false")
            .env("ATTESTED_AT", ATTESTED_AT)
            .env("POLL_SECS", "0")
            .env("POSTED_ON", posted_on)
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
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_check_that_gave_up_before_the_attestation_is_asked_again_and_the_merge_goes_on() {
    let forge = Forge::new(
        "gave-up",
        &[
            state(
                "gave-up",
                checks(&[
                    check("boundary", "fail", LONG_AGO, A_RUN),
                    check("names", "pass", LONG_AGO, A_STATUS),
                ]),
            ),
            state(
                "green",
                checks(&[
                    check("boundary", "pass", LONG_AFTER, A_RUN),
                    check("names", "pass", LONG_AGO, A_STATUS),
                ]),
            ),
        ],
        "green",
    );
    let merged = forge.merge();
    workspace::measured(1, "merge_request command read from the shipped flow");
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(
        forge.calls(),
        "run rerun 111 --failed\npr merge 7 --merge --match-head-commit abc123\n"
    );
}

#[test]
fn the_moment_is_the_one_attest_answered_not_the_clock() {
    let forge = Forge::new(
        "moment",
        &[
            state(
                "gave-up",
                checks(&[check(
                    "boundary",
                    "fail",
                    AFTER_THE_CLOCK_BEFORE_THE_ATTESTATION,
                    A_RUN,
                )]),
            ),
            state(
                "green",
                checks(&[check("boundary", "pass", LONG_AFTER, A_RUN)]),
            ),
        ],
        "green",
    );
    let merged = forge.merge();
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(
        forge.calls(),
        "run rerun 111 --failed\npr merge 7 --merge --match-head-commit abc123\n"
    );
}

#[test]
fn a_run_asked_again_is_waited_for_before_the_merge() {
    let forge = Forge::new(
        "queued",
        &[
            state(
                "gave-up",
                checks(&[check("boundary", "fail", LONG_AGO, A_RUN)]),
            ),
            State {
                name: "queued",
                checks: checks(&[check("boundary", "pending", LONG_AGO, A_RUN)]),
                moves_on: Some((3, "green")),
            },
            state(
                "green",
                checks(&[check("boundary", "pass", LONG_AFTER, A_RUN)]),
            ),
        ],
        "queued",
    );
    let merged = forge.merge();
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(
        forge.calls(),
        "run rerun 111 --failed\npr merge 7 --merge --match-head-commit abc123\n"
    );
}

#[test]
fn both_waits_share_one_hour() {
    let most = AN_HOUR_OF_POLLS * 3 / 4;
    let forge = Forge::new(
        "hour",
        &[
            State {
                name: "slow",
                checks: checks(&[check("boundary", "pending", LONG_AGO, A_RUN)]),
                moves_on: Some((most, "gave-up")),
            },
            state(
                "gave-up",
                checks(&[check("boundary", "fail", LONG_AGO, A_RUN)]),
            ),
            State {
                name: "queued",
                checks: checks(&[check("boundary", "pending", LONG_AGO, A_RUN)]),
                moves_on: Some((most, "green")),
            },
            state(
                "green",
                checks(&[check("boundary", "pass", LONG_AFTER, A_RUN)]),
            ),
        ],
        "queued",
    );
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert!(
        said(&refused).contains("still pending after an hour"),
        "{}",
        said(&refused)
    );
    assert_eq!(forge.calls(), "run rerun 111 --failed\n");
}

#[test]
fn a_check_still_red_when_asked_again_refuses_the_merge_after_one_try() {
    let forge = Forge::new(
        "still-red",
        &[state(
            "red",
            checks(&[check("boundary", "fail", LONG_AGO, A_RUN)]),
        )],
        "red",
    );
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert!(
        said(&refused).contains("not green on pull request 7: boundary"),
        "{}",
        said(&refused)
    );
    assert_eq!(forge.calls(), "run rerun 111 --failed\n");
}

#[test]
fn a_check_that_failed_after_the_attestation_is_not_asked_again() {
    let forge = Forge::new(
        "after",
        &[state(
            "red",
            checks(&[check("boundary", "fail", LONG_AFTER, A_RUN)]),
        )],
        "red",
    );
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert_eq!(forge.calls(), "");
}

#[test]
fn a_red_status_that_no_run_carries_refuses_without_asking_anything() {
    let forge = Forge::new(
        "status",
        &[state(
            "red",
            checks(&[check("names", "fail", LONG_AGO, A_STATUS)]),
        )],
        "red",
    );
    let refused = forge.merge();
    assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
    assert_eq!(forge.calls(), "");
}

#[test]
fn all_green_merges_without_asking_anything_again() {
    let forge = Forge::new(
        "green",
        &[state(
            "green",
            checks(&[check("boundary", "pass", LONG_AGO, A_RUN)]),
        )],
        "green",
    );
    let merged = forge.merge();
    assert!(merged.status.success(), "{}", said(&merged));
    assert_eq!(forge.calls(), "pr merge 7 --merge --match-head-commit abc123\n");
}

#[test]
fn without_sailor_integrated_on_the_reviewed_head_nothing_is_merged() {
    let forge = Forge::new(
        "not-integrated",
        &[state(
            "green",
            checks(&[check("boundary", "pass", LONG_AGO, A_RUN)]),
        )],
        "green",
    );
    for posted_on in ["", "another-commit"] {
        let refused = forge.merge_posted_on(posted_on);
        assert_eq!(refused.status.code(), Some(1), "{}", said(&refused));
        assert!(
            said(&refused).contains("no sailor/integrated on abc123: nothing is merged"),
            "{}",
            said(&refused)
        );
    }
    assert_eq!(forge.calls(), "");
}
