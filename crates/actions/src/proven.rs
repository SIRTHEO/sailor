//! Which finished branches the forge can prove: `close-the-work` closes a
//! branch only on its own merged pull request, and a branch without one failed
//! every run. A failed run leaves the sensor's change unconsumed, so it started
//! again after each cooldown for as long as the branch stood. Such a branch is
//! named here and never offered.

use crate::finished::as_an_item;
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROVEN_BRANCHES_ACTION: &str = "proven_branches";

const PROVEN_FIELDS: &[&str] = &["repo", "workdir", "branches"];

#[derive(Debug, Deserialize)]
struct ProvenSpec {
    repo: Option<PathBuf>,
    workdir: Option<PathBuf>,
    branches: Vec<String>,
}

fn not_proven(why: String) -> ActionError {
    ActionError::new("the_finished_branches_are_not_proven", why)
}

/// The branches offered to `close-the-work`, and the ones named instead.
pub fn split_by_proof(
    repo: &Path,
    branches: &[String],
    merged: &BTreeSet<String>,
) -> (Vec<Value>, Vec<String>) {
    let (proven, unproven): (Vec<&String>, Vec<&String>) =
        branches.iter().partition(|name| merged.contains(*name));
    (
        proven
            .into_iter()
            .map(|name| as_an_item(repo, name))
            .collect(),
        unproven.into_iter().cloned().collect(),
    )
}

/// The forge's own program, and every variable through which an inherited
/// environment would make it answer as another account or for another
/// repository. Named once, here, for ADR-020's count.
const FORGE_PROGRAM: &str = "gh";
const FORGE_TOKEN: &str = "GH_TOKEN";
const NEVER_INHERITED: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_REPO",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

fn forge_program() -> Command {
    let mut command = Command::new(FORGE_PROGRAM);
    for variable in NEVER_INHERITED {
        command.env_remove(variable);
    }
    command
}

/// Every forge call acts as the account the tree declares, never as whichever
/// account the machine has active.
fn forge(repo: &Path, arguments: &[&str]) -> Result<String, ActionError> {
    let declared = run(Command::new("git").arg("-C").arg(repo).args(["config", "--get", "sailor.forgeAs"]))
        .map_err(|_| not_proven("sailor.forgeAs is not declared on this tree: refusing to act as the machine's active account".to_owned()))?;
    let account = declared.trim();
    if account.is_empty() || account.starts_with('-') {
        return Err(not_proven(format!(
            "sailor.forgeAs is not an account: {account:?}"
        )));
    }
    let token = run(forge_program().args(["auth", "token", "--user", account]))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(not_proven(format!(
            "no forge token for the declared account {account}"
        )));
    }
    run(forge_program()
        .current_dir(repo)
        .args(arguments)
        .env(FORGE_TOKEN, token))
}

fn run(command: &mut Command) -> Result<String, ActionError> {
    let output = command
        .output()
        .map_err(|why| not_proven(format!("{:?} would not run: {why}", command.get_program())))?;
    if !output.status.success() {
        return Err(not_proven(format!(
            "{:?} answered {}: {}",
            command.get_program(),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct ProvenBranchesAction;

impl Action for ProvenBranchesAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ProvenSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let repo = spec.repo.or(spec.workdir).ok_or_else(|| {
            not_proven("no tree to read: neither a repo nor a workdir is named".to_owned())
        })?;
        // One question for the whole list, and asked only when the list
        // changed. A branch past the limit reads as unproven: it waits, and
        // nothing is deleted on a guess.
        let merged: BTreeSet<String> = forge(
            &repo,
            &[
                "pr",
                "list",
                "--state",
                "merged",
                "--limit",
                "1000",
                "--json",
                "headRefName",
                "--jq",
                ".[].headRefName",
            ],
        )?
        .lines()
        .map(str::to_owned)
        .collect();
        let (items, unproven) = split_by_proof(&repo, &spec.branches, &merged);
        Ok(ActionOutcome::Went(json!({
            "items": items,
            "unproven": unproven,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !PROVEN_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }

    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

pub fn register_proven_branches(registry: &mut flow::ActionRegistry) {
    registry.register(PROVEN_BRANCHES_ACTION, ProvenBranchesAction);
}
