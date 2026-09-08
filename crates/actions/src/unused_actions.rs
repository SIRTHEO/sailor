//! `unused_actions`: which registered actions no flow — system or the
//! project's own — ever names. Static only, and deliberately so: a step
//! behind a `when` that never fires is still *named*, so it does not appear
//! here. That is a different fault, with a different cure, and conflating
//! the two would hide one behind the other.
//!
//! **A FLOW THAT FAILS TO PARSE IS NOT A FLOW THAT NAMES NOTHING.** Reading
//! it and refusing are two different answers; treating a refusal as an empty
//! answer is the fault this tree already has a name for elsewhere
//! (`CouldNotLook`). The count below says how many flows it could actually
//! read, so a reader can tell a real audit from one taken on a smaller
//! world than it thinks.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A path no real project is ever rooted at, used only when nothing declared
/// a home: **`None` must not read as "the current directory"**, the rule
/// `flow::workspace::find_root` already states. A bare relative `"flows"`
/// would resolve against wherever the process happens to be launched from,
/// which can silently pick up an unrelated directory that happens to hold
/// one.
const NO_HOME_DECLARED: &str = "/sailor-no-home-declared";

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

    /// Every action named by some step of some flow that actually parsed,
    /// and how many did not — a flow this failed to read must not count as
    /// a flow that named nothing.
    fn referenced(&self) -> (BTreeSet<String>, usize, usize) {
        let home = self
            .home_flows
            .clone()
            .unwrap_or_else(|| Path::new(NO_HOME_DECLARED).to_path_buf());
        let known = flow::system::load_all(&flow::system::sources_from_env(&home));
        let unreadable = known.iter().filter(|(_, _, entry)| entry.is_err()).count();
        let names = known
            .iter()
            .filter_map(|(_, _, entry)| entry.as_ref().ok())
            .flat_map(|flow| flow.graph.steps().iter().map(|step| step.action.clone()))
            .collect();
        (names, known.len() - unreadable, unreadable)
    }
}

impl Action for UnusedActionsAction {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let (referenced, flows_read, flows_unreadable) = self.referenced();
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
            "flows_read": flows_read,
            "flows_unreadable": flows_unreadable,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-unused-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// **A FLOW THAT DOES NOT PARSE IS COUNTED AS UNREADABLE, NOT AS A FLOW
    /// NAMING NOTHING.** Its own action must not be able to hide behind a
    /// parse failure and read as unused.
    #[test]
    fn a_flow_that_fails_to_parse_is_declared_unreadable_not_silent() {
        let dir = scratch("broken");
        std::fs::write(dir.join("broken.flow.json"), "{ not json").expect("write it broken");
        let audited = UnusedActionsAction::new(Some(dir.clone()), vec!["trigger".to_owned()])
            .execute(&json!({}), &SharedState::default())
            .expect("a broken flow does not crash the audit");
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(said) = audited else {
            panic!("{audited:?}")
        };
        assert!(
            said["flows_unreadable"].as_u64().unwrap_or(0) >= 1,
            "{said:?}"
        );
    }

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
