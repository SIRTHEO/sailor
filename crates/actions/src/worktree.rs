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
use workspace::{OpenTrees, Worktree};

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
    NotCutBySailor(PathBuf),
    SomebodyIsIn(PathBuf, WhoIsIn),
    Locked(PathBuf),
    TakeItDown(PathBuf),
}

pub fn what_becomes_of_it(
    trees: &[Worktree],
    top: &Path,
    repo: &Path,
    branch: &str,
    terminals: &[PathBuf],
    processes: &[(u32, PathBuf)],
    written_down: &[PathBuf],
) -> WhatBecomesOfIt {
    let Some(found) = trees
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(branch))
    else {
        return WhatBecomesOfIt::NoTreeCarriesIt;
    };
    let at = PathBuf::from(&found.path);
    let here = workspace::standing::canonical(&at);
    if here == workspace::standing::canonical(top)
        || workspace::standing::canonical(repo).starts_with(&here)
    {
        return WhatBecomesOfIt::ItIsWhereTheFlowRuns(at);
    }
    if !written_down
        .iter()
        .any(|cut| workspace::standing::canonical(cut) == here)
    {
        return WhatBecomesOfIt::NotCutBySailor(at);
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

/// The one a step was given, or the machine's own.
fn which_register(given: &Option<ledger::Ledger>) -> Result<ledger::Ledger, ActionError> {
    given.clone().map_or_else(the_register, Ok)
}

/// The trees Sailor wrote down as it cut them: the only ones it takes down.
fn the_register() -> Result<ledger::Ledger, ActionError> {
    let directory = ledger::default_directory()
        .ok_or_else(|| stays("no register of trees can be found".to_owned()))?;
    ledger::Ledger::open(&directory)
        .map_err(|why| stays(format!("the register of trees cannot be opened: {why}")))
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
        let register = the_register()?;
        let written_down = register
            .trees_left_open()
            .map_err(|why| stays(format!("the register of trees cannot be read: {why}")))?
            .into_iter()
            .map(|row| PathBuf::from(row.path))
            .collect::<Vec<_>>();
        let at = match what_becomes_of_it(
            &trees,
            &top,
            &spec.repo,
            &spec.branch,
            &terminals,
            &processes,
            &written_down,
        ) {
            WhatBecomesOfIt::NoTreeCarriesIt => {
                return Ok(ActionOutcome::Went(json!({ "removed": "" })))
            }
            WhatBecomesOfIt::ItIsWhereTheFlowRuns(at) => {
                return Err(stays(format!(
                    "the tree holding {} is the one this flow runs from ({}): name another \
                     checkout of the repository as repo instead",
                    spec.branch,
                    at.display()
                )))
            }
            WhatBecomesOfIt::NotCutBySailor(at) => {
                return Err(stays(format!(
                    "{} was not cut by Sailor, so it is named and left to whoever cut it",
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
        workspace::remove_at(&spec.repo, &at, &register)
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

pub const TAKE_A_TREE_ACTION: &str = "take_a_tree";

const TAKE_FIELDS: &[&str] = &["repo", "at", "lock"];

/// The register is handed in, as the store's is: a run that writes its trees
/// down in one register and looks for them in another finds none.
pub fn register_take_a_tree(registry: &mut flow::ActionRegistry, register: Option<ledger::Ledger>) {
    registry.register(TAKE_A_TREE_ACTION, TakeATreeAction(register));
}

#[derive(Debug, Deserialize)]
struct TakeSpec {
    repo: PathBuf,
    /// The commit the tree stands on. `HEAD` of the repository when unsaid,
    /// which is what a step wanting "a tree here" means.
    #[serde(default = "here")]
    at: String,
    /// Why the tree is locked while the run works in it. Unlocked when unsaid:
    /// a lock a nobody gave a reason for is one a person cannot judge.
    #[serde(default)]
    lock: Option<String>,
}

fn here() -> String {
    "HEAD".to_owned()
}

/// **CUTTING A TREE IS A STEP THAT DOES NOTHING ELSE.** A shell that cut one and
/// then merged into it left nothing to give back when the merge conflicted: the
/// step failed with no output, and the give-back reading the path off it was
/// skipped. A tree a shell cuts is written down nowhere, and every closer
/// refuses what the register does not carry. ADR-026.
struct TakeATreeAction(Option<ledger::Ledger>);

impl Action for TakeATreeAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: TakeSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let named = |key: &str| shared.get(key).and_then(Value::as_str).unwrap_or_default();
        let (run, step) = (named(flow::CURRENT_RUN), named(flow::CURRENT_STEP));
        if run.is_empty() || step.is_empty() {
            return Err(ActionError::new(
                "tree_not_cut",
                "this run says neither which run nor which step it is, so there is no name to \
                 cut a tree under and nothing to write down",
            ));
        }
        let register = which_register(&self.0)?;
        let at = workspace::tree_for_at(
            &spec.repo,
            run,
            step,
            &spec.at,
            spec.lock.as_deref(),
            &register,
            ledger::born_second_of(std::process::id()),
        )
        .map_err(|why| ActionError::new("tree_not_cut", why))?;
        Ok(ActionOutcome::Went(json!({ "tree": at.to_string_lossy() })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !TAKE_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// A retried step is handed the tree its first attempt cut: the path is the
    /// run's and the step's, so a second cut at the same name is the same tree.
    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::SameOperation("the tree cut for this step".to_owned())
    }
}

pub const GIVE_A_TREE_BACK_ACTION: &str = "give_a_tree_back";

const GIVE_BACK_FIELDS: &[&str] = &["repo", "tree"];

pub fn register_give_a_tree_back(
    registry: &mut flow::ActionRegistry,
    register: Option<ledger::Ledger>,
) {
    registry.register(GIVE_A_TREE_BACK_ACTION, GiveATreeBackAction(register));
}

#[derive(Debug, Deserialize)]
struct GiveBackSpec {
    repo: PathBuf,
    tree: PathBuf,
}

/// **WHAT A RUN TOOK, A RUN GIVES BACK, AND THE REGISTER SEES BOTH.** The three
/// give-back steps removed their tree through a raw `worktree remove`, which
/// leaves the row standing: 22 of the register's 25 rows pointed at directories
/// that were gone. Taking down and taking off the register are one gesture, here
/// as in `sailor worktree remove`. A tree the register does not carry is named
/// and left to whoever cut it.
struct GiveATreeBackAction(Option<ledger::Ledger>);

impl Action for GiveATreeBackAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: GiveBackSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let register = which_register(&self.0)?;
        let written_down = register
            .trees_left_open()
            .map_err(|why| stays(format!("the register of trees cannot be read: {why}")))?
            .into_iter()
            .any(|row| Path::new(&row.path) == spec.tree);
        if !written_down {
            return Err(stays(format!(
                "{} is on no register of trees this run took, so it is left to whoever cut it",
                spec.tree.display()
            )));
        }
        workspace::give_back_at(&spec.repo, &spec.tree, &register).map_err(|why| {
            stays(format!(
                "git would not give {} back: {why}",
                spec.tree.display()
            ))
        })?;
        Ok(ActionOutcome::Went(
            json!({ "removed": spec.tree.to_string_lossy() }),
        ))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !GIVE_BACK_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::SameOperation("the tree this run took".to_owned())
    }
}
