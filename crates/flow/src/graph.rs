use crate::ValueSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub deps: Vec<String>,
    pub input_schema: ValueSchema,
    pub output_schema: ValueSchema,
    /// Values declared by the step win over the keys it receives as input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<Value>,
    /// Sees only the typed input value; `said` is out of reach.
    pub when: Option<Condition>,
    /// Stable name the executor resolves, not code embedded in the graph.
    pub action: String,
    /// How many times this step may **break** before the run gives up on it.
    /// Counts the first attempt; zero is not a valid value. A step answering
    /// `NotYet` spends none of these: not yet is not a failure.
    pub max_attempts: u32,
    /// Seconds before a step that answered `NotYet` is asked again.
    ///
    /// Absent means "not again inside this invocation, and no sooner than the
    /// next one" — never "at once", which would spin the executor on one step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_again_after_secs: Option<u32>,
    /// Seconds before a broken step is retried.
    ///
    /// Absent keeps what the engine has always done: it returns to the ready
    /// set in the same loop, so the attempts burn together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Equals { value: Value },
    PointerEquals { pointer: String, value: Value },
    PointerExists { pointer: String },
}

impl Condition {
    pub fn matches(&self, input: &Value) -> bool {
        match self {
            Condition::Equals { value } => input == value,
            Condition::PointerEquals { pointer, value } => input.pointer(pointer) == Some(value),
            Condition::PointerExists { pointer } => input.pointer(pointer).is_some(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "GraphData", into = "GraphData")]
pub struct Graph {
    steps: Vec<Step>,
    skippable_dependencies: BTreeSet<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GraphData {
    steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    skippable_dependencies: BTreeSet<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdge {
    pub step: String,
    pub dependency: String,
}

impl DependencyEdge {
    /// Names an edge; the graph constructor is what declares it skippable.
    pub fn new(step: impl Into<String>, dependency: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            dependency: dependency.into(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum GraphError {
    EmptyId,
    DuplicateStep(String),
    MissingDependency { step: String, dependency: String },
    DuplicateDependency { step: String, dependency: String },
    InvalidSkippableDependency { step: String, dependency: String },
    Cycle,
    ZeroAttempts(String),
    DestructiveInputOverlay { step: String },
    IncompatibleInput { step: String },
}

impl Graph {
    pub fn new(steps: Vec<Step>) -> Result<Self, GraphError> {
        Self::with_skippable_dependencies(steps, [])
    }

    pub fn with_skippable_dependencies(
        steps: Vec<Step>,
        dependencies: impl IntoIterator<Item = DependencyEdge>,
    ) -> Result<Self, GraphError> {
        let graph = Self {
            steps,
            skippable_dependencies: dependencies.into_iter().collect(),
        };
        graph.validate()?;
        Ok(graph)
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|step| step.id == id)
    }

    pub fn dependency_is_skippable(&self, step: &str, dependency: &str) -> bool {
        self.skippable_dependencies
            .contains(&DependencyEdge::new(step, dependency))
    }

    fn validate(&self) -> Result<(), GraphError> {
        let mut by_id = BTreeMap::new();
        for step in &self.steps {
            if step.id.is_empty() {
                return Err(GraphError::EmptyId);
            }
            if step.max_attempts == 0 {
                return Err(GraphError::ZeroAttempts(step.id.clone()));
            }
            if by_id.insert(step.id.as_str(), step).is_some() {
                return Err(GraphError::DuplicateStep(step.id.clone()));
            }
        }

        for edge in &self.skippable_dependencies {
            let valid = by_id.get(edge.step.as_str()).is_some_and(|step| {
                step.deps
                    .iter()
                    .any(|dependency| dependency == &edge.dependency)
            });
            if !valid {
                return Err(GraphError::InvalidSkippableDependency {
                    step: edge.step.clone(),
                    dependency: edge.dependency.clone(),
                });
            }
        }

        for step in &self.steps {
            let mut seen = BTreeSet::new();
            for dependency in &step.deps {
                if !seen.insert(dependency) {
                    return Err(GraphError::DuplicateDependency {
                        step: step.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
                if !by_id.contains_key(dependency.as_str()) {
                    return Err(GraphError::MissingDependency {
                        step: step.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
            self.validate_input_overlay(step, &by_id)?;
            if let Some(produced) = self.produced_input_schema(step, &by_id) {
                if !step.input_schema.accepts(&produced) {
                    return Err(GraphError::IncompatibleInput {
                        step: step.id.clone(),
                    });
                }
            }
        }

        let mut remaining: BTreeMap<&str, usize> = self
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step.deps.len()))
            .collect();
        let mut front: Vec<&str> = remaining
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(*id))
            .collect();
        let mut visited = 0;
        while let Some(done) = front.pop() {
            visited += 1;
            for step in &self.steps {
                if step.deps.iter().any(|dependency| dependency == done) {
                    let count = remaining
                        .get_mut(step.id.as_str())
                        .expect("every step was inserted");
                    *count -= 1;
                    if *count == 0 {
                        front.push(step.id.as_str());
                    }
                }
            }
        }
        if visited != self.steps.len() {
            return Err(GraphError::Cycle);
        }
        Ok(())
    }

    fn validate_input_overlay(
        &self,
        step: &Step,
        by_id: &BTreeMap<&str, &Step>,
    ) -> Result<(), GraphError> {
        let Some(with) = step.with.as_ref() else {
            return Ok(());
        };
        let has_required_dependency = step
            .deps
            .iter()
            .any(|dependency| !self.dependency_is_skippable(&step.id, dependency));
        if !has_required_dependency {
            return Ok(());
        }
        if !with.is_object() {
            return Err(GraphError::DestructiveInputOverlay {
                step: step.id.clone(),
            });
        }
        if let [only] = step.deps.as_slice() {
            let output_schema = &by_id
                .get(only.as_str())
                .expect("the dependency exists")
                .output_schema;
            if !self.dependency_is_skippable(&step.id, only)
                && !matches!(output_schema, ValueSchema::Object { .. } | ValueSchema::Any)
            {
                return Err(GraphError::DestructiveInputOverlay {
                    step: step.id.clone(),
                });
            }
        } else if step.deps.iter().any(|dependency| {
            !self.dependency_is_skippable(&step.id, dependency) && with.get(dependency).is_some()
        }) {
            return Err(GraphError::DestructiveInputOverlay {
                step: step.id.clone(),
            });
        }
        // A skippable dependency already promises its data may be missing, so
        // `with` may declare the stand-in the node receives instead.
        Ok(())
    }

    fn produced_input_schema(
        &self,
        step: &Step,
        by_id: &BTreeMap<&str, &Step>,
    ) -> Option<ValueSchema> {
        let produced = match step.deps.as_slice() {
            [] => None,
            [only] if !self.dependency_is_skippable(&step.id, only) => by_id
                .get(only.as_str())
                .map(|step| step.output_schema.clone()),
            many => Some(ValueSchema::object(
                many.iter().map(|id| {
                    (
                        id.clone(),
                        by_id
                            .get(id.as_str())
                            .expect("the dependencies exist")
                            .output_schema
                            .clone(),
                    )
                }),
                many.iter()
                    .filter(|id| !self.dependency_is_skippable(&step.id, id))
                    .cloned(),
            )),
        };
        match (produced, step.with.as_ref()) {
            (Some(produced), Some(with)) => Some(schema_with_overlay(produced, with)),
            (produced, _) => produced,
        }
    }
}

fn schema_with_overlay(produced: ValueSchema, with: &Value) -> ValueSchema {
    let Value::Object(with) = with else {
        return ValueSchema::OneOf {
            values: vec![with.clone()],
        };
    };
    let (mut properties, mut required, allow_extra) = match produced {
        ValueSchema::Object {
            properties,
            required,
            allow_extra,
        } => (properties, required, allow_extra),
        ValueSchema::Any => (BTreeMap::new(), BTreeSet::new(), true),
        _ => (BTreeMap::new(), BTreeSet::new(), false),
    };
    for (name, value) in with {
        properties.insert(
            name.clone(),
            ValueSchema::OneOf {
                values: vec![value.clone()],
            },
        );
        required.insert(name.clone());
    }
    ValueSchema::Object {
        properties,
        required,
        allow_extra,
    }
}

impl TryFrom<GraphData> for Graph {
    type Error = GraphError;

    fn try_from(value: GraphData) -> Result<Self, Self::Error> {
        let graph = Self {
            steps: value.steps,
            skippable_dependencies: value.skippable_dependencies,
        };
        graph.validate()?;
        Ok(graph)
    }
}

impl From<Graph> for GraphData {
    fn from(value: Graph) -> Self {
        Self {
            steps: value.steps,
            skippable_dependencies: value.skippable_dependencies,
        }
    }
}

impl std::fmt::Debug for GraphError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, out)
    }
}

impl Display for GraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::EmptyId => write!(formatter, "step id cannot be empty"),
            GraphError::DuplicateStep(id) => write!(formatter, "duplicate step {id}"),
            GraphError::MissingDependency { step, dependency } => {
                write!(
                    formatter,
                    "step {step} depends on missing step {dependency}"
                )
            }
            GraphError::DuplicateDependency { step, dependency } => {
                write!(formatter, "step {step} repeats dependency {dependency}")
            }
            GraphError::InvalidSkippableDependency { step, dependency } => write!(
                formatter,
                "step {step} marks non-dependency {dependency} as skippable"
            ),
            GraphError::Cycle => write!(formatter, "graph contains a backward dependency"),
            GraphError::ZeroAttempts(step) => write!(formatter, "step {step} allows zero attempts"),
            GraphError::DestructiveInputOverlay { step } => write!(
                formatter,
                "the `with` field of step {step} would discard the output of a required dependency"
            ),
            GraphError::IncompatibleInput { step } => {
                write!(
                    formatter,
                    "step {step} input does not accept dependency output"
                )
            }
        }
    }
}

impl Error for GraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, deps: &[&str]) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|id| (*id).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::String,
            with: None,
            when: None,
            action: id.to_owned(),
            max_attempts: 1,
            ask_again_after_secs: None,
            retry_after_secs: None,
        }
    }

    #[test]
    fn arbitrary_backward_edges_are_rejected() {
        let error = Graph::new(vec![step("a", &["b"]), step("b", &["a"])]);
        assert_eq!(error, Err(GraphError::Cycle));
    }

    #[test]
    fn join_schema_is_checked_as_dependency_object() {
        let mut join = step("join", &["left", "right"]);
        join.input_schema = ValueSchema::object(
            [
                ("left".to_owned(), ValueSchema::String),
                ("right".to_owned(), ValueSchema::String),
            ],
            ["left".to_owned(), "right".to_owned()],
        );
        assert!(Graph::new(vec![
            step("root", &[]),
            step("left", &["root"]),
            step("right", &["root"]),
            join
        ])
        .is_ok());
    }

    #[test]
    fn dependency_schema_is_merged_with_step_values() {
        let mut source = step("source", &[]);
        source.output_schema = ValueSchema::object(
            [("panel".to_owned(), ValueSchema::String)],
            ["panel".to_owned()],
        );
        let mut send = step("send", &["source"]);
        send.with = Some(serde_json::json!({"text": "/clear"}));
        send.input_schema = ValueSchema::object(
            [
                ("panel".to_owned(), ValueSchema::String),
                (
                    "text".to_owned(),
                    ValueSchema::OneOf {
                        values: vec![serde_json::json!("/clear")],
                    },
                ),
            ],
            ["panel".to_owned(), "text".to_owned()],
        );

        assert!(Graph::new(vec![source, send]).is_ok());
    }

    #[test]
    fn non_object_with_cannot_replace_required_dependency_input() {
        let mut source = step("source", &[]);
        source.output_schema = ValueSchema::Any;
        let mut send = step("send", &["source"]);
        send.with = Some(serde_json::json!("/clear"));

        assert_eq!(
            Graph::new(vec![source, send]),
            Err(GraphError::DestructiveInputOverlay {
                step: "send".to_owned(),
            })
        );
    }

    #[test]
    fn non_object_dependency_output_cannot_be_replaced_by_with() {
        let source = step("source", &[]);
        let mut send = step("send", &["source"]);
        send.with = Some(serde_json::json!({"text": "/clear"}));

        assert_eq!(
            Graph::new(vec![source, send]),
            Err(GraphError::DestructiveInputOverlay {
                step: "send".to_owned(),
            })
        );
    }

    #[test]
    fn required_join_dependency_cannot_be_replaced_by_with() {
        let panel = step("panel", &[]);
        let text = step("text", &[]);
        let mut send = step("send", &["panel", "text"]);
        send.with = Some(serde_json::json!({"text": "/clear"}));

        assert_eq!(
            Graph::new(vec![panel, text, send]),
            Err(GraphError::DestructiveInputOverlay {
                step: "send".to_owned(),
            })
        );
    }

    #[test]
    fn skippable_join_dependency_can_be_replaced_by_with() {
        let panel = step("panel", &[]);
        let text = step("text", &[]);
        let mut send = step("send", &["panel", "text"]);
        send.with = Some(serde_json::json!({"text": "/clear"}));

        assert!(Graph::with_skippable_dependencies(
            vec![panel, text, send],
            [DependencyEdge::new("send", "text")],
        )
        .is_ok());
    }
}
