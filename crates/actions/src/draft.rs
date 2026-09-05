//! A sketch becomes a flow. `action_list` hands whoever drafts the words the
//! engine answers to; `flow_draft` takes the flow they wrote and keeps it only
//! if it stands — the graph validates and every action named is real.

use flow::{Action, ActionError, ActionOutcome, FlowFile, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const ACTION_LIST_ACTION: &str = "action_list";
pub const FLOW_DRAFT_ACTION: &str = "flow_draft";

/// Registered last, so the list it hands out is the whole registry — these
/// two included.
pub fn register_draft(registry: &mut flow::ActionRegistry, flows_dir: Option<PathBuf>) {
    let mut known: Vec<String> = registry.names().into_iter().map(str::to_owned).collect();
    known.push(ACTION_LIST_ACTION.to_owned());
    known.push(FLOW_DRAFT_ACTION.to_owned());
    known.sort();
    registry.register(ACTION_LIST_ACTION, ActionListAction::new(known.clone()));
    registry.register(FLOW_DRAFT_ACTION, FlowDraftAction::new(flows_dir, known));
}

pub struct ActionListAction {
    names: Vec<String>,
}

impl ActionListAction {
    pub fn new(names: Vec<String>) -> Self {
        Self { names }
    }
}

impl Action for ActionListAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({ "actions": self.names })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[derive(Debug, Deserialize)]
struct DraftSpec {
    flow: Value,
}

pub struct FlowDraftAction {
    flows_dir: Option<PathBuf>,
    known: Vec<String>,
}

impl FlowDraftAction {
    pub fn new(flows_dir: Option<PathBuf>, known: Vec<String>) -> Self {
        Self { flows_dir, known }
    }

    /// The flow, standing, or the reason it does not.
    pub fn accept(&self, flow_json: Value) -> Result<FlowFile, ActionError> {
        let flow: FlowFile = serde_json::from_value(flow_json)
            .map_err(|error| ActionError::new("invalid_flow", error.to_string()))?;
        let unknown: Vec<&str> = flow
            .graph
            .steps()
            .iter()
            .map(|step| step.action.as_str())
            .filter(|action| !self.known.iter().any(|known| known == action))
            .collect();
        if !unknown.is_empty() {
            return Err(ActionError::new("unknown_action", unknown.join(", ")));
        }
        Ok(flow)
    }
}

impl Action for FlowDraftAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: DraftSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let flow = self.accept(spec.flow)?;
        let flows_dir = self
            .flows_dir
            .as_ref()
            .ok_or_else(|| ActionError::new("no_flows_dir", String::new()))?;
        flow::system::save_in(flows_dir, &flow)
            .map_err(|reason| ActionError::new("draft_not_written", reason))?;
        Ok(ActionOutcome::Went(json!({
            "flow": flow.id,
            "steps": flow.graph.steps().len(),
            "path": flows_dir.join(format!("{}.flow.json", flow.id)),
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_flow(action: &str) -> Value {
        json!({
            "id": "una-bozza",
            "description": "two steps, one of them named by the test",
            "graph": { "steps": [
                { "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1,
                  "with": { "source": "manual" },
                  "input_schema": { "type": "any" }, "output_schema": { "type": "any" } },
                { "id": "second", "deps": ["trigger"], "action": action, "max_attempts": 1,
                  "input_schema": { "type": "any" }, "output_schema": { "type": "any" } }
            ] },
            "inputs": { "trigger": { "text": "" } }
        })
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-draft-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn drafter(dir: &PathBuf) -> FlowDraftAction {
        FlowDraftAction::new(
            Some(dir.clone()),
            vec!["trigger".to_owned(), "work_survey".to_owned()],
        )
    }

    /// A flow whose every action is real is written where flows live, and
    /// reads back as the same flow.
    #[test]
    fn a_standing_draft_is_written_where_flows_live() {
        let dir = scratch("standing");
        let went = drafter(&dir)
            .execute(&json!({ "flow": a_flow("work_survey") }), &SharedState::default())
            .expect("the draft stands");
        let written = std::fs::read_to_string(dir.join("una-bozza.flow.json"));
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(said) = went else { panic!("{went:?}") };
        assert_eq!(said["flow"], json!("una-bozza"));
        assert_eq!(said["steps"], json!(2));
        let back: FlowFile = serde_json::from_str(&written.expect("the file")).expect("a flow");
        assert_eq!(back.id, "una-bozza");
    }

    /// **A DRAFT NAMING AN ACTION THE ENGINE DOES NOT HAVE IS NOT WRITTEN**:
    /// it would load in the window and break at its first run.
    #[test]
    fn a_draft_naming_an_unknown_action_writes_nothing() {
        let dir = scratch("unknown");
        let refused = drafter(&dir)
            .execute(&json!({ "flow": a_flow("summon_dragon") }), &SharedState::default())
            .expect_err("refused");
        let left = std::fs::read_dir(&dir).map(|entries| entries.count()).unwrap_or(0);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(refused.class, "unknown_action", "{refused:?}");
        assert!(refused.said.contains("summon_dragon"), "{refused:?}");
        assert_eq!(left, 0, "nothing on disk");
    }

    /// The graph's own rules hold before the actions are looked at.
    #[test]
    fn a_draft_with_a_missing_dependency_writes_nothing() {
        let dir = scratch("dangling");
        let mut flow = a_flow("work_survey");
        flow["graph"]["steps"][1]["deps"] = json!(["nowhere"]);
        let refused = drafter(&dir)
            .execute(&json!({ "flow": flow }), &SharedState::default())
            .expect_err("refused");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(refused.class, "invalid_flow", "{refused:?}");
    }
}
