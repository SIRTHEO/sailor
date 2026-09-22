//! A reviewer's verdict, bound to the commit that was pinned and to the sailor
//! the review was closed with.
//!
//! **A FLOW STEP IS NOT A SHELL PROGRAM.** This stood as 1.349 characters of
//! shell and inline python in `review-a-pinned-commit · verdict_bound`.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const REVIEW_VERDICT_ACTION: &str = "review_verdict";

const VERDICT_FIELDS: &[&str] = &["verdict", "commit", "sailor", "sha256"];

/// The executor reads the passing word at a pointer into the step's answer;
/// this action writes the answer, so it names that same field rather than
/// spelling it again and drifting from it, which is how a review last came to
/// end failed on a verdict it had recorded.
fn passing_field() -> &'static str {
    flow::VERDICT_FIELD.trim_start_matches('/')
}

pub fn register_review_verdict(registry: &mut flow::ActionRegistry) {
    registry.register(REVIEW_VERDICT_ACTION, ReviewVerdictAction);
}

#[derive(Debug, Deserialize)]
struct VerdictSpec {
    /// What the reviewer handed in: one object, as the review step stored it.
    verdict: Value,
    /// The commit the review was pinned to.
    commit: String,
    /// The sailor the review was to be closed with, and its sha256 when the
    /// flow verified it.
    sailor: PathBuf,
    sha256: String,
}

/// What a verdict binds when it holds: counts, because the texts stay with the
/// review itself.
#[derive(Debug, PartialEq, Eq)]
pub struct Bound {
    pub commit: String,
    pub verdict: String,
    pub findings: usize,
    pub checked: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    NotAnObject,
    OtherCommit(Option<String>),
    NeitherCleanNorFindings,
    NotLists,
    FindingsNamedNone,
    NothingChecked,
}

/// The verdict against the pinned commit, with nothing read from the machine.
pub fn what_the_verdict_binds(verdict: &Value, pinned: &str) -> Result<Bound, Refusal> {
    let fields = verdict.as_object().ok_or(Refusal::NotAnObject)?;
    let commit = fields.get("commit").and_then(Value::as_str);
    if commit != Some(pinned) {
        return Err(Refusal::OtherCommit(commit.map(str::to_owned)));
    }
    let said = fields.get("verdict").and_then(Value::as_str);
    let said = said
        .filter(|said| matches!(*said, "clean" | "findings"))
        .ok_or(Refusal::NeitherCleanNorFindings)?;
    let empty = Value::Array(Vec::new());
    let (Some(findings), Some(checked)) = (
        fields.get("findings").unwrap_or(&empty).as_array(),
        fields.get("checked").unwrap_or(&empty).as_array(),
    ) else {
        return Err(Refusal::NotLists);
    };
    if said == "findings" && findings.is_empty() {
        return Err(Refusal::FindingsNamedNone);
    }
    if checked.is_empty() {
        return Err(Refusal::NothingChecked);
    }
    Ok(Bound {
        commit: pinned.to_owned(),
        verdict: said.to_owned(),
        findings: findings.len(),
        checked: checked.len(),
    })
}

fn said(refusal: Refusal, pinned: &str) -> String {
    match refusal {
        Refusal::NotAnObject => "the verdict is not one JSON object".to_owned(),
        Refusal::OtherCommit(named) => format!(
            "the verdict names commit {}, not the pinned {pinned}",
            named.as_deref().unwrap_or("none")
        ),
        Refusal::NeitherCleanNorFindings => "the verdict is neither clean nor findings".to_owned(),
        Refusal::NotLists => "findings and checked are lists".to_owned(),
        Refusal::FindingsNamedNone => "a verdict of findings names none".to_owned(),
        Refusal::NothingChecked => "the verdict lists nothing it checked".to_owned(),
    }
}

struct ReviewVerdictAction;

impl Action for ReviewVerdictAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: VerdictSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let shown = spec.sailor.display();
        let bytes = std::fs::read(&spec.sailor).map_err(|_| {
            ActionError::new(
                "not_in_service",
                format!("cannot read the sailor the review was to be closed with, {shown}"),
            )
        })?;
        let now = format!("{:x}", Sha256::digest(&bytes));
        if now != spec.sha256 {
            return Err(ActionError::new(
                "not_in_service",
                format!(
                    "the sailor at {shown} changed while the review was open (sha256 {now}, not \
                     {}): the verdict it recorded is not taken",
                    spec.sha256
                ),
            ));
        }
        let bound = what_the_verdict_binds(&spec.verdict, &spec.commit).map_err(|refusal| {
            ActionError::new("the_verdict_is_not_bound", said(refusal, &spec.commit))
        })?;
        let mut answer = serde_json::Map::new();
        answer.insert(passing_field().to_owned(), json!(flow::VERDICT_PASSED));
        answer.insert("commit".to_owned(), json!(bound.commit));
        answer.insert("verdict".to_owned(), json!(bound.verdict));
        answer.insert("findings".to_owned(), json!(bound.findings));
        answer.insert("checked".to_owned(), json!(bound.checked));
        Ok(ActionOutcome::Went(Value::Object(answer)))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !VERDICT_FIELDS.contains(&name.as_str()))
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
