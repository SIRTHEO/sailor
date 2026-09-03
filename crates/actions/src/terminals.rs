//! The node with which a flow reads the terminals Sailor follows.
//!
//! `work_survey` answers who *announced* themselves; this answers who Sailor
//! saw open one, and the two lists differ. It never says «dead»: `closed` is a
//! terminal that said goodbye, `detached` one alive that asked not to be
//! followed. Only the store's path is taken here, nothing opened.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The name `TerminalSurveyAction` registers under.
pub const TERMINAL_SURVEY_ACTION: &str = "terminal_survey";

/// The fields the survey knows.
const KNOWN_FIELDS: &[&str] = &["store", "worktree", "at"];

pub fn register_terminals(registry: &mut flow::ActionRegistry, store: Option<PathBuf>) {
    registry.register(TERMINAL_SURVEY_ACTION, TerminalSurveyAction { store });
}

#[derive(Debug, Deserialize)]
struct SurveySpec {
    #[serde(default)]
    store: Option<String>,
    #[serde(default)]
    worktree: Option<String>,
    #[serde(default)]
    at: Option<i64>,
}

struct TerminalSurveyAction {
    store: Option<PathBuf>,
}

impl TerminalSurveyAction {
    fn where_it_reads(&self, declared: &Option<String>) -> Result<PathBuf, ActionError> {
        if let Some(written) = declared {
            return Ok(PathBuf::from(written));
        }
        self.store.clone().ok_or_else(|| {
            ActionError::new(
                "no_store",
                "I cannot tell where the terminals are written".to_owned(),
            )
        })
    }
}

impl Action for TerminalSurveyAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SurveySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let at = spec.at.unwrap_or_else(sessions::now);
        let store = self.where_it_reads(&spec.store)?;
        let sessions = sessions::Sessions::open(&store)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let rows = sessions
            .terminals()
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let mut working: Vec<Value> = Vec::new();
        let mut gone: Vec<Value> = Vec::new();
        for row in rows {
            if let Some(wanted) = &spec.worktree {
                if &row.worktree != wanted {
                    continue;
                }
            }
            let mut entry = json!({
                "tty": row.tty,
                "worktree": row.worktree,
                "ancestor": row.ancestor,
                "session_id": row.session_id,
                "opened_at": row.opened_at,
                "open_for_secs": at - row.opened_at,
            });
            match why_gone(&row) {
                None => working.push(entry),
                Some(why) => {
                    entry["why"] = json!(why);
                    gone.push(entry);
                }
            }
        }
        Ok(ActionOutcome::Went(json!({
            "at": at,
            "working": working,
            "gone": gone,
        })))
    }

    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match declared.as_object() {
            Some(fields) => fields
                .keys()
                .filter(|name| !KNOWN_FIELDS.contains(&name.as_str()))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Why a terminal no longer counts among those working, `None` while it does.
fn why_gone(row: &sessions::TerminalRow) -> Option<&'static str> {
    if row.closed_at.is_some() {
        return Some("closed");
    }
    if row.is_detached() {
        return Some("detached");
    }
    None
}
