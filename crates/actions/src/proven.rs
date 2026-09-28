//! Which finished branches the forge can prove, so that `close-the-work`,
//! which closes a branch only on its own merged request, is never offered one
//! it must fail on: a failed run leaves a sensor's change unconsumed, and the
//! flow would start again every cooldown. A missing declaration never changes
//! by itself, so it is named in `why` and the run completes; a forge that does
//! not answer may answer later, so that fails and the cooldown asks again.

use crate::finished::as_an_item;
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROVEN_BRANCHES_ACTION: &str = "proven_branches";

const PROVEN_FIELDS: &[&str] = &["repo", "workdir", "branches"];

/// More merged requests than one question reads would leave the oldest
/// finished branches unproven with nothing saying why, so reaching it refuses.
const MERGED_READ_AT_ONCE: usize = 10_000;

/// A program a descriptor declares as speaking for a forge (ADR-020): the
/// product names none, the toolbox hands them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeProgram {
    pub forge: String,
    pub program: String,
    pub token_variable: String,
    pub never_inherited: Vec<String>,
}

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

/// The program that speaks for the forge a policy declares, if a descriptor
/// declares one.
pub fn program_for<'a>(forges: &'a [ForgeProgram], forge: &str) -> Option<&'a ForgeProgram> {
    forges.iter().find(|program| program.forge == forge)
}

/// Why a tree cannot be asked at all: a declaration it lacks, a program not
/// installed, or no token held for the declared account.
fn undeclared(repo: &Path, forges: &[ForgeProgram]) -> Result<(ForgeProgram, String), String> {
    let policy = workspace::delivery::policy_on_the_trunk(repo)
        .map_err(|why| format!("the tree declares no delivery policy: {why}"))?;
    if policy.forge.is_empty() {
        return Err("the delivery policy declares no forge".to_owned());
    }
    let program = program_for(forges, &policy.forge)
        .ok_or_else(|| {
            format!(
                "no descriptor declares a program for the forge {:?}",
                policy.forge
            )
        })?
        .clone();
    let declared = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", "sailor.forgeAs"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    if declared.is_empty() || declared.starts_with('-') {
        return Err("sailor.forgeAs is not declared on this tree: no account to act as".to_owned());
    }
    let token = run(clean(&program).args(["auth", "token", "--user", &declared]))
        .map(|token| token.trim().to_owned())
        .map_err(|refused| {
            format!(
                "no forge token for the declared account {declared}: {}",
                refused.said
            )
        })?;
    if token.is_empty() {
        return Err(format!(
            "no forge token for the declared account {declared}"
        ));
    }
    Ok((program, token))
}

/// The forge's program with nothing inherited that would make it act as
/// another account or for another repository.
fn clean(program: &ForgeProgram) -> Command {
    let mut command = Command::new(&program.program);
    for variable in &program.never_inherited {
        command.env_remove(variable);
    }
    command
}

/// Every forge call acts as the account the tree declares, never as whichever
/// account the machine has active.
fn forge(
    repo: &Path,
    program: &ForgeProgram,
    token: &str,
    arguments: &[&str],
) -> Result<String, ActionError> {
    run(clean(program)
        .current_dir(repo)
        .args(arguments)
        .env(&program.token_variable, token))
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

struct ProvenBranchesAction {
    forges: Vec<ForgeProgram>,
}

impl Action for ProvenBranchesAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ProvenSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let repo = spec.repo.or(spec.workdir).ok_or_else(|| {
            not_proven("no tree to read: neither a repo nor a workdir is named".to_owned())
        })?;
        let (program, token) = match undeclared(&repo, &self.forges) {
            Ok(declared) => declared,
            Err(missing) => {
                return Ok(ActionOutcome::Went(json!({
                    "items": [],
                    "unproven": spec.branches,
                    "why": missing,
                })))
            }
        };
        let limit = MERGED_READ_AT_ONCE.to_string();
        let answered = forge(
            &repo,
            &program,
            &token,
            &[
                "pr",
                "list",
                "--state",
                "merged",
                "--limit",
                &limit,
                "--json",
                "headRefName",
                "--jq",
                ".[].headRefName",
            ],
        )?;
        let merged: BTreeSet<String> = answered.lines().map(str::to_owned).collect();
        if answered.lines().count() >= MERGED_READ_AT_ONCE {
            return Err(not_proven(format!(
                "the forge holds {MERGED_READ_AT_ONCE} merged requests or more, and one question reads no further"
            )));
        }
        let (items, unproven) = split_by_proof(&repo, &spec.branches, &merged);
        Ok(ActionOutcome::Went(json!({
            "items": items,
            "unproven": unproven,
            "why": "",
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

pub fn register_proven_branches(registry: &mut flow::ActionRegistry, forges: Vec<ForgeProgram>) {
    registry.register(PROVEN_BRANCHES_ACTION, ProvenBranchesAction { forges });
}
