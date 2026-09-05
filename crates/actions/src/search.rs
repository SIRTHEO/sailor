//! `flow_search`: the flows that mention the words asked, best first. The
//! text searched is the whole flow file — id, description, every mandate and
//! `with` — so a word written once in one step's prompt finds that flow.

use flow::{Action, ActionError, ActionOutcome, FlowFile, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FLOW_SEARCH_ACTION: &str = "flow_search";

pub fn register_search(registry: &mut flow::ActionRegistry, home_flows: Option<PathBuf>) {
    registry.register(FLOW_SEARCH_ACTION, FlowSearchAction::new(home_flows));
}

/// What a flow is known as when loaded: its name, where it came from, and the
/// file or the reason it did not load.
pub type Known = (String, &'static str, Result<FlowFile, String>);

/// The flows among `known` that mention every word of `query`, best first,
/// each with where it came from and a snippet around the match.
pub fn rank_flows(known: &[Known], query: &str) -> Result<Vec<Value>, String> {
    let documents: Vec<(String, String)> = known
        .iter()
        .filter_map(|(name, _, entry)| entry.as_ref().ok().map(|flow| (name, flow)))
        .map(|(name, flow)| (name.clone(), serde_json::to_string(flow).unwrap_or_default()))
        .collect();
    let hits = ledger::search::rank_texts(&documents, query).map_err(|error| error.to_string())?;
    Ok(hits
        .into_iter()
        .map(|hit| {
            let origin = known
                .iter()
                .find(|(name, _, _)| name == &hit.id)
                .map(|(_, origin, _)| *origin)
                .unwrap_or_default();
            json!({ "flow": hit.id, "origin": origin, "rank": hit.rank, "excerpt": hit.excerpt })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct SearchSpec {
    query: String,
}

pub struct FlowSearchAction {
    home_flows: Option<PathBuf>,
}

impl FlowSearchAction {
    pub fn new(home_flows: Option<PathBuf>) -> Self {
        Self { home_flows }
    }
}

impl Action for FlowSearchAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SearchSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let home = self.home_flows.clone().unwrap_or_else(|| Path::new("flows").to_path_buf());
        let known = flow::system::load_all(&flow::system::sources_from_env(&home));
        let hits = rank_flows(&known, &spec.query)
            .map_err(|reason| ActionError::new("search_refused", reason))?;
        Ok(ActionOutcome::Went(json!({ "query": spec.query, "hits": hits })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> Vec<Known> {
        flow::system::load_all(&[flow::system::FlowSource::builtin()])
    }

    /// The oracle is the text itself: the flows found are exactly the shipped
    /// flows whose file contains the word, and there is at least one, or the
    /// test would pass on a search that finds nothing.
    #[test]
    fn a_word_written_in_one_flow_finds_that_flow_and_no_other() {
        let known = shipped();
        let hits = rank_flows(&known, "yesterday").expect("a ranking");
        let found: Vec<&str> = hits.iter().filter_map(|hit| hit["flow"].as_str()).collect();
        let mentioning: Vec<&str> = known
            .iter()
            .filter(|(_, _, entry)| {
                entry.as_ref().is_ok_and(|flow| {
                    serde_json::to_string(flow).unwrap_or_default().contains("yesterday")
                })
            })
            .map(|(name, _, _)| name.as_str())
            .collect();
        assert_eq!(mentioning.len(), 1, "the word has to be in exactly one shipped flow: {mentioning:?}");
        assert_eq!(found, mentioning);
        assert_eq!(hits[0]["origin"], json!(flow::system::FlowSource::builtin().origin));
    }

    #[test]
    fn a_word_no_flow_says_finds_nothing() {
        let hits = rank_flows(&shipped(), "zwieback").expect("a ranking");
        assert!(hits.is_empty(), "{hits:?}");
    }
}
