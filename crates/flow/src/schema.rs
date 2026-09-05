use crate::record::{Refusal, RefusalRule};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ValueSchema {
    Any,
    Null,
    Boolean,
    Number,
    String,
    OneOf {
        values: Vec<Value>,
    },
    Array {
        items: Box<ValueSchema>,
    },
    Object {
        properties: BTreeMap<String, ValueSchema>,
        required: BTreeSet<String>,
        allow_extra: bool,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub path: String,
    pub expected: String,
    pub rule: RefusalRule,
    /// The offending value as JSON; empty when the fault is a missing field.
    pub seen: String,
}

impl SchemaError {
    /// The refusal a check named `check` records for this error.
    pub fn refused_by(&self, check: &str) -> Refusal {
        Refusal::new(check, self.path.clone(), self.rule, &self.seen)
    }
}

impl ValueSchema {
    pub fn validate(&self, value: &Value) -> Result<(), SchemaError> {
        self.validate_at(value, "$".to_owned())
    }

    /// True if this schema would accept a property under this name, so a value
    /// is offered only to what can receive it: the project root is added to the
    /// input of steps that declare no `workdir`, and adding it to a step with a
    /// closed schema kills it on "declared property". Measured on the trigger
    /// step of `sviluppa-sailor`, which is closed and has nothing whatever to
    /// do with a working directory.
    pub fn accepts_property(&self, name: &str) -> bool {
        match self {
            ValueSchema::Any => true,
            ValueSchema::Object {
                properties,
                allow_extra,
                ..
            } => *allow_extra || properties.contains_key(name),
            _ => false,
        }
    }

    /// Whether every value the second schema produces is accepted by the first.
    pub fn accepts(&self, produced: &ValueSchema) -> bool {
        match (self, produced) {
            (ValueSchema::Any, _) => true,
            (_, ValueSchema::Any) => matches!(self, ValueSchema::Any),
            (ValueSchema::Null, ValueSchema::Null)
            | (ValueSchema::Boolean, ValueSchema::Boolean)
            | (ValueSchema::Number, ValueSchema::Number)
            | (ValueSchema::String, ValueSchema::String) => true,
            (ValueSchema::Null, ValueSchema::OneOf { values }) => values.iter().all(Value::is_null),
            (ValueSchema::Boolean, ValueSchema::OneOf { values }) => {
                values.iter().all(Value::is_boolean)
            }
            (ValueSchema::Number, ValueSchema::OneOf { values }) => {
                values.iter().all(Value::is_number)
            }
            (ValueSchema::String, ValueSchema::OneOf { values }) => {
                values.iter().all(Value::is_string)
            }
            (ValueSchema::OneOf { values: wanted }, ValueSchema::OneOf { values: actual }) => {
                actual.iter().all(|value| wanted.contains(value))
            }
            (ValueSchema::Array { items: wanted }, ValueSchema::Array { items: actual }) => {
                wanted.accepts(actual)
            }
            // A list written inside a step arrives here as "this exact value",
            // not as "a list": without this arm, a field that wants a list
            // refuses the only way there is to write one. Three flows became
            // "invalid" together the day steps stopped naming a single engine
            // and declared a chain instead — the cause was not in the flows.
            // The arms for strings, numbers and booleans were already here.
            (ValueSchema::Array { items }, ValueSchema::OneOf { values }) => {
                values.iter().all(|value| {
                    value
                        .as_array()
                        .is_some_and(|list| list.iter().all(|item| items.validate(item).is_ok()))
                })
            }
            (
                ValueSchema::Object {
                    properties: wanted,
                    required,
                    allow_extra,
                },
                ValueSchema::Object {
                    properties: actual,
                    required: actual_required,
                    allow_extra: actual_extra,
                },
            ) => {
                required.is_subset(actual_required)
                    && wanted.iter().all(|(name, schema)| {
                        actual.get(name).is_some_and(|found| schema.accepts(found))
                    })
                    && (*allow_extra
                        || (!actual_extra && actual.keys().all(|key| wanted.contains_key(key))))
            }
            _ => false,
        }
    }

    pub fn object(
        properties: impl IntoIterator<Item = (String, ValueSchema)>,
        required: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::Object {
            properties: properties.into_iter().collect(),
            required: required.into_iter().collect(),
            allow_extra: false,
        }
    }

    fn validate_at(&self, value: &Value, path: String) -> Result<(), SchemaError> {
        let mismatch = || SchemaError {
            path: path.clone(),
            expected: self.kind().to_owned(),
            rule: RefusalRule::WrongType,
            seen: value.to_string(),
        };
        match self {
            ValueSchema::Any => Ok(()),
            ValueSchema::Null if value.is_null() => Ok(()),
            ValueSchema::Boolean if value.is_boolean() => Ok(()),
            ValueSchema::Number if value.is_number() => Ok(()),
            ValueSchema::String if value.is_string() => Ok(()),
            ValueSchema::OneOf { values } if values.contains(value) => Ok(()),
            ValueSchema::OneOf { values } => Err(SchemaError {
                path,
                expected: format!("one of {}; found {}", Value::Array(values.clone()), value),
                rule: RefusalRule::NotAllowed,
                seen: value.to_string(),
            }),
            ValueSchema::Array { items } => {
                let values = value.as_array().ok_or_else(mismatch)?;
                for (index, item) in values.iter().enumerate() {
                    items.validate_at(item, format!("{path}[{index}]"))?;
                }
                Ok(())
            }
            ValueSchema::Object {
                properties,
                required,
                allow_extra,
            } => {
                let values = value.as_object().ok_or_else(mismatch)?;
                if let Some(missing) = required.iter().find(|key| !values.contains_key(*key)) {
                    return Err(SchemaError {
                        path: format!("{path}.{missing}"),
                        expected: "required property".to_owned(),
                        rule: RefusalRule::MissingField,
                        seen: String::new(),
                    });
                }
                for (name, item) in values {
                    match properties.get(name) {
                        Some(schema) => schema.validate_at(item, format!("{path}.{name}"))?,
                        None if !allow_extra => {
                            return Err(SchemaError {
                                path: format!("{path}.{name}"),
                                expected: "declared property".to_owned(),
                                rule: RefusalRule::UnknownField,
                                seen: item.to_string(),
                            });
                        }
                        None => {}
                    }
                }
                Ok(())
            }
            _ => Err(mismatch()),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            ValueSchema::Any => "any value",
            ValueSchema::Null => "null",
            ValueSchema::Boolean => "boolean",
            ValueSchema::Number => "number",
            ValueSchema::String => "string",
            ValueSchema::OneOf { .. } => "one of the declared values",
            ValueSchema::Array { .. } => "array",
            ValueSchema::Object { .. } => "object",
        }
    }
}

impl std::fmt::Debug for SchemaError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, out)
    }
}

impl Display for SchemaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: expected {}", self.path, self.expected)
    }
}

impl Error for SchemaError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn closed_set_rejects_and_names_the_found_value() {
        let schema = ValueSchema::OneOf {
            values: vec![json!("keep"), json!("remove")],
        };

        let error = schema
            .validate(&json!("remvoe"))
            .expect_err("unknown value");
        let message = error.to_string();
        assert!(message.contains("remvoe"), "{message}");
        assert!(message.contains("keep"), "{message}");
        assert!(message.contains("remove"), "{message}");
    }

    /// A field that wants a list of strings accepts the list written inside a
    /// step, which arrives here as "this exact value". Without it, a step
    /// cannot declare an engine chain such as `"tool": ["claude-code", "agy"]`
    /// at all — the arms for strings and numbers have always been there.
    #[test]
    fn a_field_that_wants_a_list_accepts_the_list_written_in_a_step() {
        let wanted = ValueSchema::Array {
            items: Box::new(ValueSchema::String),
        };

        let written = ValueSchema::OneOf {
            values: vec![json!(["claude-code", "agy"])],
        };

        assert!(wanted.accepts(&written));
    }

    /// And refuses what is not a list, or holds something that is not a string:
    /// a fallback declared with a number inside must not pass here and break at
    /// execution time.
    #[test]
    fn a_field_that_wants_a_list_of_strings_refuses_anything_else() {
        let wanted = ValueSchema::Array {
            items: Box::new(ValueSchema::String),
        };

        assert!(!wanted.accepts(&ValueSchema::OneOf {
            values: vec![json!("claude-code")],
        }));
        assert!(!wanted.accepts(&ValueSchema::OneOf {
            values: vec![json!(["claude-code", 7])],
        }));
    }

    /// Each way a value can fail names its rule, the field, and what was there,
    /// so a refusal can be counted by rule instead of read from prose.
    #[test]
    fn every_way_of_failing_names_its_rule_its_field_and_what_it_saw() {
        let schema = ValueSchema::Object {
            properties: [
                (
                    "verdict".to_owned(),
                    ValueSchema::OneOf {
                        values: vec![json!("keep"), json!("remove")],
                    },
                ),
                ("count".to_owned(), ValueSchema::Number),
            ]
            .into_iter()
            .collect(),
            required: ["verdict".to_owned()].into_iter().collect(),
            allow_extra: false,
        };
        let refused = |value: Value| {
            schema
                .validate(&value)
                .expect_err("the value is off shape")
                .refused_by("answer_shape")
        };

        let missing = refused(json!({"count": 1}));
        assert_eq!(
            (missing.rule, missing.path.as_str(), missing.seen.as_str()),
            (RefusalRule::MissingField, "$.verdict", "")
        );
        let not_allowed = refused(json!({"verdict": "remvoe"}));
        assert_eq!(
            (not_allowed.rule, not_allowed.path.as_str(), not_allowed.seen.as_str()),
            (RefusalRule::NotAllowed, "$.verdict", "\"remvoe\"")
        );
        let wrong_type = refused(json!({"verdict": "keep", "count": "three"}));
        assert_eq!(
            (wrong_type.rule, wrong_type.path.as_str(), wrong_type.seen.as_str()),
            (RefusalRule::WrongType, "$.count", "\"three\"")
        );
        let unknown = refused(json!({"verdict": "keep", "extra": [1, 2]}));
        assert_eq!(
            (unknown.rule, unknown.path.as_str(), unknown.seen.as_str()),
            (RefusalRule::UnknownField, "$.extra", "[1,2]")
        );
        assert_eq!(unknown.check, "answer_shape");
    }
}
