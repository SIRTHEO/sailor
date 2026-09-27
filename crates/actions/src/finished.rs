//! The branches whose work is over: under the work prefix, carried by the
//! trunk, and held by no tree. Measured on the machine Sailor runs on, 36 of
//! 45 were finished and 13 of those 36 were still a checkout's branch — taking
//! one of those down is taking a working tree's ground out from under it, so a
//! held branch is named and never offered. Remote branches are not read: the
//! forge deletes its own on the merge, and none was left there to close.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FINISHED_BRANCHES_ACTION: &str = "finished_branches";

const FINISHED_FIELDS: &[&str] = &["repo", "prefix"];

#[derive(Debug, Deserialize)]
struct FinishedSpec {
    repo: PathBuf,
    /// Which branches are the flow's business. Only these are ever named.
    prefix: String,
}

fn not_read(why: String) -> ActionError {
    ActionError::new("the_finished_branches_are_not_read", why)
}

/// A word that would reach git as a flag is not a name: everything here is
/// read from a flow or from git, and a leading `-` would be an option.
fn named_not_flagged(what: &str, word: &str) -> Result<(), ActionError> {
    if word.starts_with('-') || word.is_empty() {
        return Err(not_read(format!("{what} is not a name: {word:?}")));
    }
    Ok(())
}

/// The branches `git branch --merged` lists, by name alone.
///
/// **THE MARKER IS THE WHOLE POINT.** git writes `*` for the branch this
/// checkout is on and **`+` for one another worktree holds**, and a reading
/// that trimmed both would hand back, as free to delete, the ground thirteen
/// working trees are standing on.
pub fn merged_branches(listed: &str) -> Vec<(String, bool)> {
    listed
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let held = line.starts_with('+');
            let name = line.trim_start_matches(['*', '+', ' ']);
            (name.to_owned(), held)
        })
        .collect()
}

/// Every branch a worktree of this repository has checked out, read from
/// `git worktree list --porcelain`. The `+` marker is trusted for the
/// branches it marks; this is the second reading that catches the rest.
pub fn branches_trees_hold(listed: &str) -> BTreeSet<String> {
    listed
        .lines()
        .filter_map(|line| line.strip_prefix("branch "))
        .filter_map(|name| name.trim().strip_prefix("refs/heads/"))
        .map(str::to_owned)
        .collect()
}

/// The mandate `close-the-work` reads, as the text of its trigger: one element
/// of the list `for_each` runs it for.
pub fn as_an_item(repo: &Path, branch: &str) -> Value {
    json!({
        "source": "manual",
        "text": json!({"repo": repo.to_string_lossy(), "branch": branch}).to_string(),
    })
}

fn git(repo: &Path, arguments: &[&str]) -> Result<String, ActionError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(arguments)
        .output()
        .map_err(|why| not_read(format!("git {} would not run: {why}", arguments[0])))?;
    if !output.status.success() {
        // An exit is checked before an answer is read: a lost reading is not
        // an empty list, and an empty list would close nothing at all.
        return Err(not_read(format!(
            "git {} exited {}: {}",
            arguments[0],
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct FinishedBranchesAction;

impl Action for FinishedBranchesAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: FinishedSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        named_not_flagged("the prefix", &spec.prefix)?;
        // Which remote and which trunk are the tree's to declare, not the
        // flow's to name (ADR-020): both are read where `sailor policy` reads.
        let remote = workspace::delivery::policy_on_the_trunk(&spec.repo)
            .map_err(|why| not_read(format!("the tree declares no policy: {why}")))?
            .remote;
        let base = workspace::declared_trunk(&spec.repo).map_err(not_read)?;
        named_not_flagged("the remote", &remote)?;
        named_not_flagged("the trunk", &base)?;
        git(&spec.repo, &["fetch", "-q", "--no-tags", &remote])?;
        let on_the_remote = format!("refs/remotes/{remote}/{base}");
        let trunk = git(&spec.repo, &["rev-parse", &on_the_remote])?
            .trim()
            .to_owned();
        let held = branches_trees_hold(&git(&spec.repo, &["worktree", "list", "--porcelain"])?);
        let listed = git(
            &spec.repo,
            &[
                "branch",
                "--merged",
                &on_the_remote,
                "--list",
                &format!("{}*", spec.prefix),
            ],
        )?;
        let mut free = Vec::new();
        let mut kept = Vec::new();
        let mut items = Vec::new();
        for (name, marked) in merged_branches(&listed) {
            if marked || held.contains(&name) {
                kept.push(name);
                continue;
            }
            items.push(as_an_item(&spec.repo, &name));
            free.push(name);
        }
        Ok(ActionOutcome::Went(json!({
            "trunk": trunk,
            "branches": free,
            "held_by_a_tree": kept,
            "items": items,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !FINISHED_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// It fetches and reads refs, and writes nothing a person would miss: the
    /// flow that closes finished work starts here, so it has to be a reading a
    /// sensor may take.
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

pub fn register_finished_branches(registry: &mut flow::ActionRegistry) {
    registry.register(FINISHED_BRANCHES_ACTION, FinishedBranchesAction);
}
