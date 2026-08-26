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
    /// Riceve soltanto il valore tipato in ingresso; `said` non è raggiungibile.
    pub when: Option<Condition>,
    /// Nome stabile dell'azione risolta dall'esecutore, non codice incorporato nel grafo.
    pub action: String,
    /// Include il primo tentativo; zero non è un valore valido.
    pub max_attempts: u32,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GraphData {
    steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    EmptyId,
    DuplicateStep(String),
    MissingDependency { step: String, dependency: String },
    DuplicateDependency { step: String, dependency: String },
    Cycle,
    ZeroAttempts(String),
    IncompatibleInput { step: String },
}

impl Graph {
    pub fn new(steps: Vec<Step>) -> Result<Self, GraphError> {
        let graph = Self { steps };
        graph.validate()?;
        Ok(graph)
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|step| step.id == id)
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
                        .expect("tutti i passi sono stati inseriti");
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

    fn produced_input_schema(
        &self,
        step: &Step,
        by_id: &BTreeMap<&str, &Step>,
    ) -> Option<ValueSchema> {
        match step.deps.as_slice() {
            [] => None,
            [only] => by_id
                .get(only.as_str())
                .map(|step| step.output_schema.clone()),
            many => Some(ValueSchema::object(
                many.iter().map(|id| {
                    (
                        id.clone(),
                        by_id
                            .get(id.as_str())
                            .expect("le dipendenze esistono")
                            .output_schema
                            .clone(),
                    )
                }),
                many.iter().cloned(),
            )),
        }
    }
}

impl TryFrom<GraphData> for Graph {
    type Error = GraphError;

    fn try_from(value: GraphData) -> Result<Self, Self::Error> {
        Self::new(value.steps)
    }
}

impl From<Graph> for GraphData {
    fn from(value: Graph) -> Self {
        Self { steps: value.steps }
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
            GraphError::Cycle => write!(formatter, "graph contains a backward dependency"),
            GraphError::ZeroAttempts(step) => write!(formatter, "step {step} allows zero attempts"),
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
            when: None,
            action: id.to_owned(),
            max_attempts: 1,
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
}
