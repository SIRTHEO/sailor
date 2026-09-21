//! The binary in service, and whether anybody vouched for it.
//!
//! **A FLOW STEP IS NOT A SHELL PROGRAM.** This stood as 1.416 characters of
//! shell in three flow files, spelling out by hand where Sailor's home is —
//! `ledger::sailor_home`, which diverges as soon as a copy of it exists — and
//! where a release leaves the binary with its stamp: `release::TARGETS`.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const SAILOR_IN_SERVICE_ACTION: &str = "sailor_in_service";

const IN_SERVICE_FIELDS: &[&str] = &["home"];

/// The release row named on the command line, which carries where the binary
/// in service lives and where its stamp is written.
const THE_BINARY: &str = "sailor";

pub fn register_in_service(registry: &mut flow::ActionRegistry) {
    registry.register(SAILOR_IN_SERVICE_ACTION, SailorInServiceAction);
}

#[derive(Debug, Deserialize)]
struct InServiceSpec {
    /// The home to judge, for whoever judges one other than this process's.
    /// Unsaid, it is the rule every other reader of a home goes through.
    #[serde(default)]
    home: Option<PathBuf>,
}

/// The first word of the first line that is neither blank nor a comment: how a
/// release writes a stamp, and how a person records a digest.
fn first_word_in(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| line.split_whitespace().next())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|about| about.is_file() && about.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    path.is_file()
}

/// Nothing proceeds on a binary nobody vouched for. The class is written here
/// rather than held in a constant: the scan that pairs every class with a
/// sentence reads the literal at the call, and a constant is a blind spot.
fn refused(why: String) -> ActionError {
    ActionError::new("not_in_service", why)
}

/// A file that has to be there, told apart from one that is there and unread:
/// the first says nobody put it there, the second that something is wrong with
/// this machine, and a flow stopping on either wants to say which.
fn read_or_refuse(path: &Path, absent: String, unread: String) -> Result<String, ActionError> {
    if !path.exists() {
        return Err(refused(absent));
    }
    std::fs::read_to_string(path).map_err(|_| refused(unread))
}

struct SailorInServiceAction;

impl Action for SailorInServiceAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: InServiceSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let target = release::TARGETS
            .iter()
            .find(|target| target.name == THE_BINARY)
            .ok_or_else(|| refused(format!("no release target is named {THE_BINARY}")))?;
        let home = match spec.home {
            Some(said) => said,
            None => ledger::sailor_home().ok_or_else(|| {
                refused(
                    "the environment names no home for Sailor, so there is no binary to look for"
                        .to_owned(),
                )
            })?,
        };
        let binary = home.join(target.safe_rel);
        let shown = binary.display();
        if !runnable(&binary) {
            return Err(refused(format!(
                "no sailor in service at {shown}: this flow calls no other"
            )));
        }
        let stamp = first_word_in(&read_or_refuse(
            &home.join(target.stamp_rel),
            format!("the sailor at {shown} has no stamp naming the commit it was built from"),
            format!("cannot read the stamp of {shown}"),
        )?);
        if stamp.is_empty() {
            return Err(refused(format!("the stamp of {shown} names no commit")));
        }
        let vouched_for = home.join(format!("{}.sha256", target.stamp_rel));
        let recorded = first_word_in(&read_or_refuse(
            &vouched_for,
            format!(
                "no sha256 is recorded for the sailor at {shown}, so nothing vouches for it. A \
                 person who trusts that binary records it with: shasum -a 256 < {shown} | awk \
                 '{{ print $1 }}' > {}",
                vouched_for.display()
            ),
            format!("cannot read the recorded sha256 of {shown}"),
        )?);
        let bytes = std::fs::read(&binary).map_err(|_| refused(format!("cannot read {shown}")))?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if recorded.is_empty() || recorded != digest {
            return Err(refused(format!(
                "the sailor at {shown} has sha256 {digest}, and {} is recorded for it: it is not \
                 the binary that was put into service",
                if recorded.is_empty() {
                    "nothing".to_owned()
                } else {
                    recorded
                }
            )));
        }
        Ok(ActionOutcome::Went(json!({
            "sailor": binary.to_string_lossy(),
            "commit": stamp,
            "sha256": digest,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !IN_SERVICE_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}
