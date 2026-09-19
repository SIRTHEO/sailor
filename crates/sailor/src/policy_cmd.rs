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

/// The three settings the policy file declares, in the order printed.
const FIELDS: &[&str] = &["merge", "push", "release"];

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
    let commit = git(&root, &["rev-parse", "main"])
        .map_err(|_| no_policy_on_trunk())?
        .trim()
        .to_owned();
    let text =
        git(&root, &["show", "main:.sailor/delivery-policy.json"]).map_err(|_| no_policy_on_trunk())?;
    let policy: Value = serde_json::from_str(&text).map_err(|_| no_policy_on_trunk())?;

    let mut lines = Vec::with_capacity(FIELDS.len() + 1);
    for field in FIELDS {
        let value = policy.get(field).ok_or_else(no_policy_on_trunk)?;
        let word = value.as_str().ok_or_else(|| invalid_field(field, value))?;
        if word != "auto" && word != "ask" {
            return Err(invalid_field(field, value));
        }
        lines.push(format!("{field}: {word}"));
    }
    lines.push(commit);
    Ok(lines.join("\n"))
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

fn invalid_field(field: &str, value: &Value) -> String {
    catalogue::say(
        "cli.policy.invalid_field",
        &[("field", field), ("value", &value.to_string())],
    )
}
