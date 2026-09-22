//! The head of a delivered branch kept, under `archive/<branch>`, before the
//! branch itself comes down. **A FLOW STEP IS NOT A SHELL PROGRAM**: written
//! into `close-the-work` this was another thousand characters on the step that
//! deletes the remote branch. Which host the account lives on is not written
//! here (ADR-020): the tree declares the command line that prints a token in
//! `sailor.pushToken`, the way it declares its index server.

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

/// Where a tree declares the command line that prints the token of an account,
/// the way `sailor.pushAs` declares who pushes: words split on whitespace, the
/// account appended as the last word.
pub const PUSH_TOKEN: &str = "sailor.pushToken";

/// How a push over this remote is bound to an account. A path or a `file://`
/// carries no credentials; anything else is bound to `sailor.pushAs` through
/// the declared token command, and a remote this action cannot bind that way
/// is refused rather than pushed to as whoever the machine happens to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HowItIsBound {
    NothingToBind,
    AsThisAccount(Vec<String>),
    Unbindable(String),
}

pub fn how_it_is_bound(url: &str, push_as: Option<&str>, token: Option<&str>) -> HowItIsBound {
    if url.starts_with('/') || url.starts_with("file://") {
        return HowItIsBound::NothingToBind;
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return HowItIsBound::Unbindable(url.to_owned());
    }
    match (push_as, token) {
        (Some(who), Some(line)) if !who.is_empty() && line.split_whitespace().next().is_some() => {
            let mut argv: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
            argv.push(who.to_owned());
            HowItIsBound::AsThisAccount(argv)
        }
        _ => HowItIsBound::Unbindable(url.to_owned()),
    }
}

/// The token reaches git through the environment, never through a command
/// line: an argument is readable by every process on the machine.
const TOKEN_IN_THE_ENVIRONMENT: &str = "SAILOR_PUSH_TOKEN";

const HELPER: &str = concat!(
    "credential.helper=!printf \"username=x-access-token\\npassword=%s\\n\" ",
    "\"$SAILOR_PUSH_TOKEN\""
);

fn token_from(argv: &[String]) -> Result<String, ActionError> {
    let (command, arguments) = argv
        .split_first()
        .ok_or_else(|| not_archived(format!("{PUSH_TOKEN} declares no command")))?;
    let output = Command::new(command)
        .args(arguments)
        .output()
        .map_err(|why| not_archived(format!("{PUSH_TOKEN} would not run: {why}")))?;
    let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || token.is_empty() {
        return Err(not_archived(format!(
            "no token for the account that pushes: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(token)
}

fn git_at(repo: &Path, token: Option<&str>) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo);
    if let Some(token) = token {
        command.env(TOKEN_IN_THE_ENVIRONMENT, token);
        command.arg("-c").arg("credential.helper=");
        command.arg("-c").arg(HELPER);
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
        let url = read_config(&spec.repo, &format!("remote.{}.url", spec.remote))
            .ok_or_else(|| not_archived(format!("the tree has no remote named {}", spec.remote)))?;
        let bound = how_it_is_bound(
            &url,
            read_config(&spec.repo, "sailor.pushAs").as_deref(),
            read_config(&spec.repo, PUSH_TOKEN).as_deref(),
        );
        let token = match &bound {
            HowItIsBound::Unbindable(url) => {
                return Err(not_archived(format!(
                    "a push to {} at {url} cannot be bound to an account: it is bound over \
                     http(s), through sailor.pushAs and {PUSH_TOKEN}, and one of the three is \
                     missing",
                    spec.remote
                )))
            }
            HowItIsBound::NothingToBind => None,
            HowItIsBound::AsThisAccount(argv) => Some(token_from(argv)?),
        };
        let tag = tag_for(&spec.branch);
        let listed = git_at(&spec.repo, token.as_deref())
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
                let pushed = git_at(&spec.repo, token.as_deref())
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
