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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub path: String,
    pub expected: String,
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
                    });
                }
                for (name, item) in values {
                    match properties.get(name) {
                        Some(schema) => schema.validate_at(item, format!("{path}.{name}"))?,
                        None if !allow_extra => {
                            return Err(SchemaError {
                                path: format!("{path}.{name}"),
                                expected: "declared property".to_owned(),
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
}
