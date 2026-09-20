//! **A FLOW STEP IS NOT A SHELL PROGRAM.** Both of these stood in four flow
//! files as a string of shell — 1.966 characters and 314 — unparsed, untested,
//! and failing by quoting or by timeout. Goal #47. Neither reads the working
//! tree and neither trusts a binary on the path: `workspace::delivery` holds
//! the reading, and `sailor policy` reads through it too.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const DELIVERY_POLICY_ACTION: &str = "delivery_policy";
pub const THE_PERSON_SAID_ACTION: &str = "the_person_said";

const POLICY_FIELDS: &[&str] = &["repo"];
const SAID_FIELDS: &[&str] = &["said", "key"];

pub fn register_delivery(registry: &mut flow::ActionRegistry) {
    registry.register(DELIVERY_POLICY_ACTION, DeliveryPolicyAction);
    registry.register(THE_PERSON_SAID_ACTION, ThePersonSaidAction);
}

fn unknown_among(declared: &Value, known: &[&str]) -> Vec<String> {
    match declared.as_object() {
        Some(fields) => fields
            .keys()
            .filter(|name| !known.contains(&name.as_str()))
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
struct PolicySpec {
    repo: PathBuf,
}

struct DeliveryPolicyAction;

impl Action for DeliveryPolicyAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: PolicySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let policy = workspace::delivery::policy_on_the_trunk(&spec.repo)
            .map_err(|why| ActionError::new("no_trusted_policy", why.to_string()))?;
        let said = serde_json::to_value(&policy)
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        Ok(ActionOutcome::Went(said))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, POLICY_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}

#[derive(Debug, Deserialize)]
struct SaidSpec {
    #[serde(default)]
    said: Value,
    key: String,
}

/// A reference hands on a value or its JSON, and the flows spell it both ways.
fn as_answer(said: &Value) -> Value {
    match said.as_str() {
        Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        None => said.clone(),
    }
}

/// **SILENCE IS NOT A YES, AND NEITHER IS ANYTHING BUT `true`.** A missing
/// answer, one that is not an object, and the string `"true"` all stop the
/// flow: none of them is a person having said yes.
struct ThePersonSaidAction;

impl Action for ThePersonSaidAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SaidSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        if as_answer(&spec.said).get(&spec.key) != Some(&Value::Bool(true)) {
            return Err(ActionError::new(
                "not_authorized",
                format!(
                    "the person did not write {}: true; nothing proceeds",
                    spec.key
                ),
            ));
        }
        Ok(ActionOutcome::Went(json!({ "confirmed": true })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        unknown_among(declared, SAID_FIELDS)
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    fn redo_evidence(&self, _record: &flow::StepRecord) -> flow::RedoEvidence {
        flow::RedoEvidence::TouchesNothing
    }
}
