//! Taking down the tree a branch was worked in, once the work is over.
//!
//! **A FLOW STEP IS NOT A SHELL PROGRAM.** This stood as 3.626 characters in
//! one flow file: an awk program to find the tree, an lsof scan, and a second
//! reading of the terminals through a python program piped into a binary the
//! step had to vouch for first. Fault 267 is the same decision, asked thrice.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use workspace::standing::WhoIsIn;
use workspace::Worktree;

pub const CLOSE_THE_WORKTREE_ACTION: &str = "close_the_worktree";

const CLOSE_FIELDS: &[&str] = &["repo", "branch"];

/// The terminals Sailor tracks, and every process of this machine with the
/// directory it stands in.
type WhoHoldsWhat = (Vec<PathBuf>, Vec<(u32, PathBuf)>);

pub fn register_close_the_worktree(registry: &mut flow::ActionRegistry) {
    registry.register(CLOSE_THE_WORKTREE_ACTION, CloseTheWorktreeAction);
}

#[derive(Debug, Deserialize)]
struct CloseSpec {
    repo: PathBuf,
    branch: String,
}

/// What the flow does about the tree carrying a branch, decided from readings
/// already taken. Apart from the taking-down on purpose: every refusal here is
/// provable without a machine that has trees, terminals and processes on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatBecomesOfIt {
    NoTreeCarriesIt,
    ItIsWhereTheFlowRuns(PathBuf),
    SomebodyIsIn(PathBuf, WhoIsIn),
    Locked(PathBuf),
    TakeItDown(PathBuf),
}

pub fn what_becomes_of_it(
    trees: &[Worktree],
    top: &Path,
    branch: &str,
    terminals: &[PathBuf],
    processes: &[(u32, PathBuf)],
) -> WhatBecomesOfIt {
    let Some(found) = trees
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(branch))
    else {
        return WhatBecomesOfIt::NoTreeCarriesIt;
    };
    let at = PathBuf::from(&found.path);
    if workspace::standing::canonical(&at) == workspace::standing::canonical(top) {
        return WhatBecomesOfIt::ItIsWhereTheFlowRuns(at);
    }
    match workspace::standing::who_is_in(&at, terminals, processes) {
        WhoIsIn::Nobody => {}
        somebody => return WhatBecomesOfIt::SomebodyIsIn(at, somebody),
    }
    if found.locked {
        return WhatBecomesOfIt::Locked(at);
    }
    WhatBecomesOfIt::TakeItDown(at)
}

/// The tree stays. The class is written here rather than held in a constant:
/// the scan that pairs every class with a sentence reads the literal at the
/// call, and a constant is a blind spot.
fn stays(why: String) -> ActionError {
    ActionError::new("the_tree_stays", why)
}

/// Who stands where, gathered once. **A READING THAT CANNOT ANSWER COUNTS AS
/// SOMEBODY THERE**: a tree is taken down on a yes, never on a silence.
fn who_holds_what() -> Result<WhoHoldsWhat, ActionError> {
    let terminals = machine::who_is_standing().ok_or_else(|| {
        stays(
            "the terminals Sailor tracks cannot be read, so an open one cannot be ruled out"
                .to_owned(),
        )
    })?;
    let processes = machine::where_processes_stand()
        .map_err(|why| stays(format!("no process of this machine can be placed: {why}")))?;
    Ok((terminals, processes))
}

struct CloseTheWorktreeAction;

impl Action for CloseTheWorktreeAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: CloseSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let trees = workspace::list(&spec.repo).map_err(|why| stays(format!("git: {why}")))?;
        let top = trees
            .first()
            .map(|main| PathBuf::from(&main.path))
            .unwrap_or_else(|| spec.repo.clone());
        let (terminals, processes) = who_holds_what()?;
        let at = match what_becomes_of_it(&trees, &top, &spec.branch, &terminals, &processes) {
            WhatBecomesOfIt::NoTreeCarriesIt => {
                return Ok(ActionOutcome::Went(json!({ "removed": "" })))
            }
            WhatBecomesOfIt::ItIsWhereTheFlowRuns(at) => {
                return Err(stays(format!(
                    "the tree holding {} is the one this flow runs from ({}): name the primary \
                     checkout as repo instead",
                    spec.branch,
                    at.display()
                )))
            }
            WhatBecomesOfIt::SomebodyIsIn(at, WhoIsIn::ATerminal(taken)) => {
                return Err(stays(format!(
                    "an open terminal is recorded in {} ({})",
                    at.display(),
                    taken.display()
                )))
            }
            WhatBecomesOfIt::SomebodyIsIn(at, WhoIsIn::AProcess(pid)) => {
                return Err(stays(format!(
                    "process {pid} is working in {}",
                    at.display()
                )))
            }
            WhatBecomesOfIt::SomebodyIsIn(at, WhoIsIn::Nobody) => {
                return Err(stays(format!(
                    "{} is held by nobody nameable",
                    at.display()
                )))
            }
            WhatBecomesOfIt::Locked(at) => {
                return Err(stays(format!(
                    "the tree {} is locked: whoever holds it unlocks it when they have finished",
                    at.display()
                )))
            }
            WhatBecomesOfIt::TakeItDown(at) => at,
        };
        workspace::remove_at(&spec.repo, &at)
            .map_err(|why| stays(format!("git would not take {} down: {why}", at.display())))?;
        Ok(ActionOutcome::Went(
            json!({ "removed": at.to_string_lossy() }),
        ))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !CLOSE_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// A tree taken down twice is one taken down once: the second run finds no
    /// tree carrying the branch and answers that nothing was removed.
    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::SameOperation("the tree carrying the branch".to_owned())
    }
}
