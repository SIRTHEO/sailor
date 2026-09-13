//! Digesting a flow value, so a mandate's exact words can be recorded once and
//! checked again later without keeping the words twice.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};

pub const DIGEST_ACTION: &str = "digest";

const DIGEST_FIELDS: &[&str] = &["value"];

pub fn register_digest(registry: &mut flow::ActionRegistry) {
    registry.register(DIGEST_ACTION, DigestAction);
}

#[derive(Debug, Deserialize)]
struct DigestSpec {
    value: Value,
}

struct DigestAction;

impl Action for DigestAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: DigestSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("digest_incomplete", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "digest": flow::digest_input(&spec.value),
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !DIGEST_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// A hash of what is already in hand buys nothing from anywhere.
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_value_digests_the_same_and_a_different_one_does_not() {
        let action = DigestAction;
        let shared = SharedState::new();
        let one = action
            .execute(&json!({"value": "ack me"}), &shared)
            .expect("digesting a plain string");
        let two = action
            .execute(&json!({"value": "ack me"}), &shared)
            .expect("digesting it again");
        let different = action
            .execute(&json!({"value": "ack someone else"}), &shared)
            .expect("digesting a different value");
        let ActionOutcome::Went(one) = one else {
            panic!("{one:?}")
        };
        let ActionOutcome::Went(two) = two else {
            panic!("{two:?}")
        };
        let ActionOutcome::Went(different) = different else {
            panic!("{different:?}")
        };
        assert_eq!(one, two, "the same value always digests the same");
        assert_ne!(one, different, "a different value digests differently");
    }

    #[test]
    fn an_unknown_field_is_named_not_silently_accepted() {
        let action = DigestAction;
        assert_eq!(
            action.unknown_fields(&json!({"value": "x", "surprise": 1})),
            vec!["surprise".to_owned()]
        );
        assert!(action.unknown_fields(&json!({"value": "x"})).is_empty());
    }
}
