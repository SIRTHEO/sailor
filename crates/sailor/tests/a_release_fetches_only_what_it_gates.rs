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

/// Any version tag, not only the one being cut: the previous tag is read from
/// them, so one that disagrees with the remote would give the notes a wrong
/// starting point.
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

/// A git call that talks to a remote: its verb, and the first word after the
/// verb and its flags, the remote it reaches. A line parts into commands at
/// shell punctuation, so a call inside `$(...)` is seen (fault 273), and quotes
/// are dropped rather than parted at, so `--force-with-lease="a:b"` stays a flag.
fn git_call_in(line: &str) -> Option<(String, String)> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    line.split(|c: char| "()`;|&".contains(c)).find_map(|command| {
        let words: Vec<String> = command
            .split_whitespace()
            .map(|word| word.replace(['"', '\''], ""))
            .collect();
        let git = words.iter().position(|word| word == "git")?;
        let verb = git
            + 1
            + words[git + 1..]
                .iter()
                .position(|word| matches!(word.as_str(), "fetch" | "ls-remote" | "push"))?;
        let remote = words[verb + 1..].iter().find(|word| !word.starts_with('-'))?;
        Some((words[verb].clone(), remote.clone()))
    })
}

fn remote_named_in(line: &str) -> Option<String> {
    git_call_in(line).map(|(_, remote)| remote)
}

#[test]
fn no_shipped_step_reaches_a_remote_by_a_name_it_was_not_given() {
    let system = workspace_root().join("crates/flow/system");
    let mut named = Vec::new();
    let mut read = 0;
    for entry in std::fs::read_dir(&system).expect("the shipped flows") {
        let path = entry.expect("an entry").path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(flow) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        for step in flow["graph"]["steps"].as_array().into_iter().flatten() {
            let command = step["with"]["command"].as_str().unwrap_or_default();
            read += usize::from(!command.is_empty());
            let literal = command.lines().any(|line| {
                remote_named_in(line).as_deref() == Some("origin") || line.contains("refs/remotes/origin/")
            });
            if literal {
                named.push(format!("{}: {}", path.display(), step["id"]));
            }
        }
    }
    workspace::measured(read, "shell steps of the shipped flows read for a remote");
    assert!(
        named.is_empty(),
        "steps that name the remote instead of reading it: {named:#?}"
    );
    for (line, remote) in [
        ("git fetch --quiet origin", Some("origin")),
        (r#"listed=$(git -C "$REPO" ls-remote origin "$1") || exit 2"#, Some("origin")),
        (r#"echo "fetch failed"; git -C "$REPO" fetch -q origin"#, Some("origin")),
        ("git push --push-option=ci.skip origin main", Some("origin")),
        (
            r#"git "$@" push --force-with-lease="refs/heads/$BRANCH:$remote" origin"#,
            Some("origin"),
        ),
        (
            r#"git "$@" push --force-with-lease="refs/heads/$BRANCH:$remote" "$REMOTE_NAME""#,
            Some("$REMOTE_NAME"),
        ),
        (r#"git fetch -q x || { echo "cannot push origin by hand"; }"#, Some("x")),
        ("# git fetch origin, as a comment", None),
    ] {
        assert_eq!(remote_named_in(line).as_deref(), remote, "{line}");
    }
}

/// No step reads a tag it fetches whole, yet whether such a fetch takes the
/// tags is the machine's git configuration to say: under `tagOpt = --tags` a
/// tag the remote rewrote fails it with nothing on stderr. Every whole fetch
/// says it leaves the tags where they are.
#[test]
fn no_shipped_fetch_follows_the_remote_tags() {
    let system = workspace_root().join("crates/flow/system");
    let mut following = Vec::new();
    let mut fetches = 0;
    for entry in std::fs::read_dir(&system).expect("the shipped flows") {
        let path = entry.expect("an entry").path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(flow) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        for step in flow["graph"]["steps"].as_array().into_iter().flatten() {
            let command = step["with"]["command"].as_str().unwrap_or_default();
            for line in command.lines() {
                let fetch = git_call_in(line).is_some_and(|(verb, _)| verb == "fetch");
                fetches += usize::from(fetch);
                if fetch && !line.contains("--no-tags") {
                    following.push(format!("{}: {}", path.display(), step["id"]));
                }
            }
        }
    }
    workspace::measured(fetches, "fetches of a whole remote in the shipped flows");
    assert!(
        following.is_empty(),
        "steps whose fetch follows the remote tags: {following:#?}"
    );
}
