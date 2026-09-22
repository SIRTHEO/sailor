//! The head of a delivered branch kept, under `archive/<branch>`, before the
//! branch itself comes down.
//!
//! **A FLOW STEP IS NOT A SHELL PROGRAM.** Written in `close-the-work` this
//! was another thousand characters on the step that deletes the remote branch,
//! the credential helper of `sailor.pushAs` quoted a second time inside a
//! string. Here the decision is a function three tests can ask, and the push
//! is the only line that needs a remote.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ARCHIVE_THE_HEAD_ACTION: &str = "archive_the_head";

const ARCHIVE_FIELDS: &[&str] = &["repo", "remote", "branch", "head"];

#[derive(Debug, Deserialize)]
struct ArchiveSpec {
    repo: PathBuf,
    remote: String,
    branch: String,
    head: String,
}

/// Nothing is archived. The class is written at the call, not held in a
/// constant, so the scan that pairs every class with a sentence sees it.
fn not_archived(why: String) -> ActionError {
    ActionError::new("the_head_is_not_archived", why)
}

/// The tag a branch is kept under. The whole branch name goes in: two shapes
/// of it are already on this remote because the name was trimmed by hand.
pub fn tag_for(branch: &str) -> String {
    format!("refs/tags/archive/{branch}")
}

/// What becomes of the archive tag, read from what the remote lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatBecomesOfTheTag {
    PushIt,
    AlreadyThere,
    NamesAnother(String),
}

/// An annotated tag lists twice, as the tag object and as the commit it peels
/// to: the head counts as archived when it is either of them.
pub fn what_becomes_of_the_tag(listed: &str, tag: &str, head: &str) -> WhatBecomesOfTheTag {
    let peeled = format!("{tag}^{{}}");
    let named: Vec<&str> = listed
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, name)| *name == tag || *name == peeled)
        .map(|(object, _)| object)
        .collect();
    match named.first() {
        None => WhatBecomesOfTheTag::PushIt,
        Some(_) if named.contains(&head) => WhatBecomesOfTheTag::AlreadyThere,
        Some(first) => WhatBecomesOfTheTag::NamesAnother((*first).to_owned()),
    }
}

/// How a push over this remote is bound to an account. A path or a `file://`
/// carries no credentials; anything that is neither that nor http(s) cannot be
/// bound to `sailor.pushAs`, so this action refuses it rather than pushing as
/// whoever the machine happens to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HowItIsBound {
    NothingToBind,
    AsThisAccount(String),
    Unbindable(String),
}

pub fn how_it_is_bound(url: &str, push_as: Option<&str>) -> HowItIsBound {
    if url.starts_with('/') || url.starts_with("file://") {
        return HowItIsBound::NothingToBind;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return HowItIsBound::Unbindable(url.to_owned());
    }
    match push_as {
        Some(who) if !who.is_empty() => HowItIsBound::AsThisAccount(who.to_owned()),
        _ => HowItIsBound::Unbindable(url.to_owned()),
    }
}

/// The helper git is handed for one call: it prints the token of the named
/// account and nothing else, so the push never reaches for the machine's own.
fn helper_for(who: &str) -> String {
    format!(
        "!f() {{ printf \"%s\\n\" username=x-access-token; printf \"password=%s\\n\" \
         \"$(env -u GH_TOKEN -u GITHUB_TOKEN -u GH_REPO gh auth token --user \"{who}\")\"; }}; f"
    )
}

fn git_at(repo: &Path, bound: &HowItIsBound) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo);
    if let HowItIsBound::AsThisAccount(who) = bound {
        command.arg("-c").arg("credential.helper=");
        command
            .arg("-c")
            .arg(format!("credential.helper={}", helper_for(who)));
    }
    command
}

fn read_config(repo: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", key])
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !said.is_empty()).then_some(said)
}

struct ArchiveTheHeadAction;

impl Action for ArchiveTheHeadAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ArchiveSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if spec.head.is_empty() {
            return Err(not_archived(
                "no head was proved, so there is nothing to keep the branch under".to_owned(),
            ));
        }
        let url = read_config(&spec.repo, &format!("remote.{}.url", spec.remote)).ok_or_else(
            || not_archived(format!("the tree has no remote named {}", spec.remote)),
        )?;
        let bound = how_it_is_bound(&url, read_config(&spec.repo, "sailor.pushAs").as_deref());
        if let HowItIsBound::Unbindable(url) = &bound {
            return Err(not_archived(format!(
                "{} is {url}: a push over it cannot be bound to sailor.pushAs, so it is refused",
                spec.remote
            )));
        }
        let tag = tag_for(&spec.branch);
        let listed = git_at(&spec.repo, &bound)
            .args(["ls-remote", &spec.remote, &tag])
            .output()
            .map_err(|why| not_archived(format!("git ls-remote would not run: {why}")))?;
        if !listed.status.success() {
            // A lost transport is not an empty answer: nothing is decided on it.
            return Err(not_archived(format!(
                "cannot read {tag} on {}: {}",
                spec.remote,
                String::from_utf8_lossy(&listed.stderr).trim()
            )));
        }
        let said = String::from_utf8_lossy(&listed.stdout);
        match what_becomes_of_the_tag(&said, &tag, &spec.head) {
            WhatBecomesOfTheTag::AlreadyThere => {}
            WhatBecomesOfTheTag::NamesAnother(object) => {
                return Err(not_archived(format!(
                    "{tag} on {} names {object}, not the head {} this run proved: an archive tag \
                     is not moved",
                    spec.remote, spec.head
                )))
            }
            WhatBecomesOfTheTag::PushIt => {
                let pushed = git_at(&spec.repo, &bound)
                    .args(["push", &spec.remote, &format!("{}:{tag}", spec.head)])
                    .output()
                    .map_err(|why| not_archived(format!("git push would not run: {why}")))?;
                if !pushed.status.success() {
                    return Err(not_archived(format!(
                        "{tag} would not go up on {}: {}",
                        spec.remote,
                        String::from_utf8_lossy(&pushed.stderr).trim()
                    )));
                }
            }
        }
        Ok(ActionOutcome::Went(
            json!({ "tag": tag, "archived": spec.head }),
        ))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !ARCHIVE_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// A tag pushed twice is a tag pushed once: the second run finds the head
    /// already there and pushes nothing.
    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::SameOperation("the archive tag of the branch".to_owned())
    }
}

pub fn register_archive_the_head(registry: &mut flow::ActionRegistry) {
    registry.register(ARCHIVE_THE_HEAD_ACTION, ArchiveTheHeadAction);
}
