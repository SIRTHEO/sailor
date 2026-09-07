//! `unused_actions`: which registered actions no flow — system or the
//! project's own — ever names. Static only, and deliberately so: a step
//! behind a `when` that never fires is still *named*, so it does not appear
//! here. That is a different fault, with a different cure, and conflating
//! the two would hide one behind the other.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const UNUSED_ACTIONS_ACTION: &str = "unused_actions";

/// Registered after everything else, the same way `action_list` is: the list
/// it audits is the whole registry, itself included.
pub fn register_unused_actions(registry: &mut flow::ActionRegistry, home_flows: Option<PathBuf>) {
    let mut registered: Vec<String> = registry.names().into_iter().map(str::to_owned).collect();
    registered.push(UNUSED_ACTIONS_ACTION.to_owned());
    registered.sort();
    registry.register(
        UNUSED_ACTIONS_ACTION,
        UnusedActionsAction::new(home_flows, registered),
    );
}

pub struct UnusedActionsAction {
    home_flows: Option<PathBuf>,
    registered: Vec<String>,
}

impl UnusedActionsAction {
    pub fn new(home_flows: Option<PathBuf>, registered: Vec<String>) -> Self {
        Self {
            home_flows,
            registered,
        }
    }

    /// Every action named by some step of some flow, system or the
    /// project's own — the set a name must miss to count as unused.
    fn referenced(&self) -> BTreeSet<String> {
        let home = self
            .home_flows
            .clone()
            .unwrap_or_else(|| Path::new("flows").to_path_buf());
        flow::system::load_all(&flow::system::sources_from_env(&home))
            .iter()
            .filter_map(|(_, _, entry)| entry.as_ref().ok())
            .flat_map(|flow| flow.graph.steps().iter().map(|step| step.action.clone()))
            .collect()
    }
}

impl Action for UnusedActionsAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let referenced = self.referenced();
        let never_referenced: Vec<&str> = self
            .registered
            .iter()
            .map(String::as_str)
            .filter(|name| !referenced.contains(*name))
            .collect();
        Ok(ActionOutcome::Went(json!({
            "never_referenced": never_referenced,
            "registered": self.registered.len(),
            "referenced": referenced.len(),
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A NAME NO SHIPPED FLOW SPELLS IS UNUSED; ONE EVERY FLOW SPELLS IS NOT.**
    /// `trigger` opens every system flow; `summon_dragon` names nothing real,
    /// the same fake name `draft.rs`'s own tests refuse to write.
    #[test]
    fn a_name_no_flow_ever_says_is_the_one_reported() {
        let audited =
            UnusedActionsAction::new(None, vec!["trigger".to_owned(), "summon_dragon".to_owned()])
                .execute(&json!({}), &SharedState::default())
                .expect("a static count does not fail");
        let ActionOutcome::Went(said) = audited else {
            panic!("{audited:?}")
        };
        let never_referenced = said["never_referenced"].as_array().expect("a list");
        assert!(
            never_referenced.iter().any(|name| name == "summon_dragon"),
            "{said:?}"
        );
        assert!(
            !never_referenced.iter().any(|name| name == "trigger"),
            "{said:?}"
        );
    }
}
