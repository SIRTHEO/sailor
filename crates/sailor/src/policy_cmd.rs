//! `sailor policy`: the delivery policy as the trusted trunk commits it,
//! never as the branch under review would have it read — a proposed change
//! must never authorize itself. See `docs/the-delivery-loop.md`.

use crate::Form;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

pub const USAGE: &[Form] = &[Form {
    form: "sailor policy",
    says_key: "",
}];

/// **THE ONE FACT THAT CANNOT LIVE IN THE POLICY FILE**, because the file is
/// read *from* the trunk and so the trunk's name has to arrive first. It is a
/// key of the repository's own configuration, like `sailor.forgeAs` and
/// `sailor.pushAs`, and nothing is assumed when it is absent (ADR-020).
const TRUNK: &str = "sailor.trunk";

/// The three settings the policy file governs, in the order printed.
const GOVERNED: &[&str] = &["merge", "push", "release"];

/// What the repository declares about itself, printed as read. The product
/// keeps no list of the forges or the remotes that exist, so there is no
/// vocabulary to check these against — only whether they were declared.
const DECLARED: &[&str] = &["forge", "remote"];

pub fn run(_args: &[String]) -> i32 {
    match dispatch() {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(why) => {
            eprintln!("sailor policy: {why}");
            1
        }
    }
}

fn dispatch() -> Result<String, String> {
    let root = workspace::root().map_err(|_| no_policy_on_trunk())?;
    let trunk = trunk_of(&root)?;
    let commit = git(&root, &["rev-parse", &trunk])
        .map_err(|_| unknown_trunk(&trunk))?
        .trim()
        .to_owned();
    let at = format!("{commit}:.sailor/delivery-policy.json");
    let text = git(&root, &["show", &at]).map_err(|_| no_policy_on_trunk())?;
    let policy: Value = serde_json::from_str(&text).map_err(|_| no_policy_on_trunk())?;

    let mut lines = Vec::with_capacity(GOVERNED.len() + DECLARED.len() + 1);
    for field in GOVERNED {
        let value = policy.get(field).ok_or_else(no_policy_on_trunk)?;
        let word = value.as_str().ok_or_else(|| invalid_field(field, value))?;
        if word != "auto" && word != "ask" {
            return Err(invalid_field(field, value));
        }
        lines.push(format!("{field}: {word}"));
    }
    for field in DECLARED {
        match policy.get(field).and_then(Value::as_str) {
            Some(word) => lines.push(format!("{field}: {word}")),
            None => lines.push(format!("{field}: {}", not_declared())),
        }
    }
    lines.push(commit);
    Ok(lines.join("\n"))
}

/// `git config --get` exits 1 for a key nobody set and otherwise for a
/// configuration it could not read: the two are kept apart, so a repository
/// that declared nothing is told which declaration is missing rather than
/// being refused with no reason given.
fn trunk_of(repo: &Path) -> Result<String, String> {
    let read = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", TRUNK])
        .output()
        .map_err(|error| error.to_string())?;
    if read.status.code() == Some(1) {
        return Err(no_trunk_declared());
    }
    if !read.status.success() {
        return Err(String::from_utf8_lossy(&read.stderr).trim().to_owned());
    }
    let name = String::from_utf8_lossy(&read.stdout).trim().to_owned();
    if name.is_empty() {
        return Err(no_trunk_declared());
    }
    Ok(name)
}

/// A thin wrapper over `git -C <repo> <args>`: the one command this file runs,
/// always against the repository's trusted trunk, never the working tree.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn no_policy_on_trunk() -> String {
    catalogue::say("cli.policy.no_policy_on_trunk", &[])
}

fn no_trunk_declared() -> String {
    catalogue::say("cli.policy.no_trunk_declared", &[("key", TRUNK)])
}

fn unknown_trunk(trunk: &str) -> String {
    catalogue::say(
        "cli.policy.unknown_trunk",
        &[("trunk", trunk), ("key", TRUNK)],
    )
}

fn not_declared() -> String {
    catalogue::say("cli.policy.not_declared", &[])
}

fn invalid_field(field: &str, value: &Value) -> String {
    catalogue::say(
        "cli.policy.invalid_field",
        &[("field", field), ("value", &value.to_string())],
    )
}
