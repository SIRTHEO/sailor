//! Flow definitions: steps, dependencies, which action each step calls. Pure —
//! it takes a `FlowFile` or a refusal reason already in memory and never reads
//! from disk (that is `gather::load_flow_registry`'s job).

// The file format lives in the flow crate: it is imported here, never
// redeclared. Writing it twice once made the two agree by luck rather than by
// construction.
pub use flow::FlowFile;
use serde::Serialize;
use std::collections::BTreeMap;

pub type FlowRegistry = BTreeMap<String, Result<FlowFile, String>>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepView {
    pub id: String,
    pub deps: Vec<String>,
    pub action: String,
    pub max_attempts: u32,
    pub conditional: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlowView {
    pub name: String,
    pub description: Option<String>,
    pub steps: Vec<StepView>,
    pub error: Option<String>,
}

pub fn flow_view(name: &str, flow: &FlowFile) -> FlowView {
    FlowView {
        name: name.to_owned(),
        description: Some(flow.description.clone()),
        steps: flow
            .graph
            .steps()
            .iter()
            .map(|step| StepView {
                id: step.id.clone(),
                deps: step.deps.clone(),
                action: step.action.clone(),
                max_attempts: step.max_attempts,
                conditional: step.when.is_some(),
            })
            .collect(),
        error: None,
    }
}

pub fn broken_flow_view(name: &str, reason: &str) -> FlowView {
    FlowView {
        name: name.to_owned(),
        description: None,
        steps: Vec::new(),
        error: Some(reason.to_owned()),
    }
}

/// The order is the key order: alphabetical, stable from one call to the next.
/// Reports both the valid flows and the unloadable ones with their reason.
pub fn flow_views(registry: &FlowRegistry) -> Vec<FlowView> {
    registry
        .iter()
        .map(|(name, entry)| match entry {
            Ok(flow) => flow_view(name, flow),
            Err(reason) => broken_flow_view(name, reason),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{Condition, Graph, Step, ValueSchema};
    use serde_json::json;

    fn step(id: &str, deps: &[&str], conditional: bool) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::Any,
            with: None,
            when: conditional.then(|| Condition::Equals { value: json!(true) }),
            action: format!("{id}_action"),
            max_attempts: 2,
        }
    }

    fn sample_flow(id: &str, steps: Vec<Step>) -> FlowFile {
        let graph = Graph::new(steps).expect("valid graph");
        FlowFile {
            id: id.to_owned(),
            description: format!("Description for {id}"),
            graph,
            inputs: BTreeMap::new(),
            // A flow launched by hand: what is under test here is the shape of
            // the steps, never when it would run by itself.
            schedule: None,
            // No cap: these tests spend nothing.
            spend_cap_micros: None,
        }
    }

    #[test]
    fn a_flow_file_becomes_a_view_with_deps_action_description_and_conditional_flag() {
        let flow = sample_flow(
            "sample",
            vec![step("root", &[], false), step("child", &["root"], true)],
        );
        let view = flow_view("sample", &flow);
        assert_eq!(view.name, "sample");
        assert_eq!(view.description.as_deref(), Some("Description for sample"));
        assert_eq!(view.error, None);
        assert_eq!(view.steps.len(), 2);
        let child = view
            .steps
            .iter()
            .find(|s| s.id == "child")
            .expect("step present");
        assert_eq!(child.deps, vec!["root".to_owned()]);
        assert_eq!(child.action, "child_action");
        assert_eq!(child.max_attempts, 2);
        assert!(child.conditional);
        let root = view
            .steps
            .iter()
            .find(|s| s.id == "root")
            .expect("step present");
        assert!(!root.conditional);
    }

    #[test]
    fn a_broken_flow_becomes_a_view_with_error_and_empty_steps() {
        let view = broken_flow_view("broken", "dependency 'missing' is absent from the graph");
        assert_eq!(view.name, "broken");
        assert_eq!(view.description, None);
        assert_eq!(
            view.error.as_deref(),
            Some("dependency 'missing' is absent from the graph")
        );
        assert!(view.steps.is_empty());
    }

    #[test]
    fn the_registry_lists_valid_and_broken_flows_in_alphabetical_order() {
        let flow_zeta = sample_flow("zeta", vec![step("root", &[], false)]);
        let flow_alfa = sample_flow("alfa", vec![step("root", &[], false)]);
        let mut registry = FlowRegistry::new();
        registry.insert("zeta".to_owned(), Ok(flow_zeta));
        registry.insert(
            "beta".to_owned(),
            Err("the graph contains a cycle".to_owned()),
        );
        registry.insert("alfa".to_owned(), Ok(flow_alfa));

        let views = flow_views(&registry);
        assert_eq!(views.len(), 3);
        assert_eq!(views[0].name, "alfa");
        assert_eq!(views[0].error, None);
        assert_eq!(views[0].steps.len(), 1);

        assert_eq!(views[1].name, "beta");
        assert_eq!(
            views[1].error.as_deref(),
            Some("the graph contains a cycle")
        );
        assert!(views[1].steps.is_empty());

        assert_eq!(views[2].name, "zeta");
        assert_eq!(views[2].error, None);
        assert_eq!(views[2].steps.len(), 1);
    }

    #[test]
    fn flow_file_deserializes_the_expected_schema_with_id_description_graph_and_inputs() {
        let raw = json!({
            "id": "prima-corsa",
            "description": "Test flow",
            "graph": {
                "steps": [{
                    "id": "step-1",
                    "deps": [],
                    "action": "check",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {
                "step-1": { "key": "value" }
            }
        });
        let flow: FlowFile = serde_json::from_value(raw).expect("deserialization succeeded");
        assert_eq!(flow.id, "prima-corsa");
        assert_eq!(flow.description, "Test flow");
        assert_eq!(flow.graph.steps().len(), 1);
        assert!(flow.inputs.contains_key("step-1"));
    }

    #[test]
    fn flow_file_rejects_a_naked_graph_missing_top_level_metadata() {
        let naked_graph = json!({
            "steps": [{
                "id": "step-1",
                "deps": [],
                "action": "check",
                "max_attempts": 1,
                "when": null,
                "input_schema": {"type": "any"},
                "output_schema": {"type": "any"}
            }]
        });
        let result: Result<FlowFile, _> = serde_json::from_value(naked_graph);
        assert!(result.is_err(), "a bare graph must be refused: the {{ id, description, graph, inputs }} wrapper is missing");
    }
}
