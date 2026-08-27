//! Le definizioni dei flussi: passi, dipendenze, quale azione chiama ciascun
//! passo. Pura — prende un `flow::Graph` già costruito, non ne cerca uno sul
//! disco (quello è compito di `gather::load_flow_registry`).

use flow::Graph;
use serde::Serialize;
use std::collections::BTreeMap;

pub type FlowRegistry = BTreeMap<String, Graph>;

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
    pub steps: Vec<StepView>,
}

pub fn flow_view(name: &str, graph: &Graph) -> FlowView {
    FlowView {
        name: name.to_owned(),
        steps: graph
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
    }
}

/// L'ordine è quello delle chiavi: alfabetico, stabile fra una chiamata e l'altra.
pub fn flow_views(registry: &FlowRegistry) -> Vec<FlowView> {
    registry
        .iter()
        .map(|(name, graph)| flow_view(name, graph))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{Condition, Step, ValueSchema};
    use serde_json::json;

    fn step(id: &str, deps: &[&str], conditional: bool) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::Any,
            when: conditional.then(|| Condition::Equals { value: json!(true) }),
            action: format!("{id}_action"),
            max_attempts: 2,
        }
    }

    #[test]
    fn a_graph_becomes_a_view_with_deps_action_and_the_conditional_flag() {
        let graph = Graph::new(vec![step("root", &[], false), step("child", &["root"], true)])
            .expect("grafo valido");
        let view = flow_view("prova", &graph);
        assert_eq!(view.name, "prova");
        assert_eq!(view.steps.len(), 2);
        let child = view.steps.iter().find(|s| s.id == "child").expect("passo presente");
        assert_eq!(child.deps, vec!["root".to_owned()]);
        assert_eq!(child.action, "child_action");
        assert_eq!(child.max_attempts, 2);
        assert!(child.conditional);
        let root = view.steps.iter().find(|s| s.id == "root").expect("passo presente");
        assert!(!root.conditional);
    }

    #[test]
    fn the_registry_is_listed_in_alphabetical_order_of_its_names() {
        let graph = Graph::new(vec![step("root", &[], false)]).expect("grafo valido");
        let mut registry = FlowRegistry::new();
        registry.insert("zeta".to_owned(), graph.clone());
        registry.insert("alfa".to_owned(), graph);
        let views = flow_views(&registry);
        assert_eq!(views[0].name, "alfa");
        assert_eq!(views[1].name, "zeta");
    }
}
