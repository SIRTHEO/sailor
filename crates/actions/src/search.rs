//! `flow_search`: the flows that mention the words asked, best first. The
//! text searched is the whole flow file — id, description, every mandate and
//! `with` — so a word written once in one step's prompt finds that flow.

use crate::memory::{tree_name, Memory, MEMORIES_COLLECTION};
use faults::Faults;
use flow::{Action, ActionError, ActionOutcome, FlowFile, SharedState, StepSpecies};
use ledger::search::Hit;
use ledger::Ledger;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FLOW_SEARCH_ACTION: &str = "flow_search";
pub const LEDGER_SEARCH_ACTION: &str = "ledger_search";

/// How far back the ledger is read for a search: the recent runs, steps and
/// events.
pub const RECENT_RUNS: usize = 500;
pub const RECENT_STEPS: usize = 2000;
pub const RECENT_EVENTS: usize = 2000;

pub fn register_search(
    registry: &mut flow::ActionRegistry,
    home_flows: Option<PathBuf>,
    ledger: Option<Ledger>,
    faults_store: Option<PathBuf>,
) {
    registry.register(FLOW_SEARCH_ACTION, FlowSearchAction::new(home_flows));
    registry.register(LEDGER_SEARCH_ACTION, LedgerSearchAction::new(ledger, faults_store));
}

/// What the ledger and the fault register hold, ranked once over the union:
/// the ids say which kind each hit is. A register that has no file yet is a
/// register with nothing in it, not an error.
pub fn search_the_ledger_and_the_faults(
    ledger: &Ledger,
    faults_store: Option<&Path>,
    query: &str,
) -> Result<Vec<Hit>, String> {
    let mut documents = ledger
        .documents_to_search(RECENT_RUNS, RECENT_STEPS, RECENT_EVENTS)
        .map_err(|error| error.to_string())?;
    if let Some(path) = faults_store.filter(|path| path.exists()) {
        let register = Faults::open(path).map_err(|error| error.to_string())?;
        documents.extend(register.documents_to_search().map_err(|error| error.to_string())?);
    }
    ledger::search::rank_texts(&documents, query).map_err(|error| error.to_string())
}

/// The memory a hit stands for, when its id names one: the label and the tree
/// the id alone does not say. `None` for every other kind of hit.
pub fn memory_behind(ledger: &Ledger, hit: &Hit) -> Option<Memory> {
    let key = hit
        .id
        .strip_prefix("store:")?
        .strip_prefix(MEMORIES_COLLECTION)?
        .strip_prefix('/')?;
    ledger
        .read_record(MEMORIES_COLLECTION, key)
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_value(record.value).ok())
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

/// The runs, steps, events and store entries of the ledger, and the faults of
/// the register, that mention the words.
pub struct LedgerSearchAction {
    ledger: Option<Ledger>,
    faults_store: Option<PathBuf>,
}

impl LedgerSearchAction {
    pub fn new(ledger: Option<Ledger>, faults_store: Option<PathBuf>) -> Self {
        Self { ledger, faults_store }
    }
}

impl Action for LedgerSearchAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: SearchSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let ledger = self
            .ledger
            .as_ref()
            .ok_or_else(|| ActionError::new("no_store", String::new()))?;
        let hits = search_the_ledger_and_the_faults(ledger, self.faults_store.as_deref(), &spec.query)
            .map_err(|reason| ActionError::new("search_refused", reason))?;
        let hits: Vec<Value> = hits
            .into_iter()
            .map(|hit| {
                let memory = memory_behind(ledger, &hit).map(|memory| {
                    json!({ "label": memory.label, "tree": memory.tree.as_deref().map(tree_name) })
                });
                let mut found = json!({ "id": hit.id, "rank": hit.rank, "excerpt": hit.excerpt });
                if let Some(memory) = memory {
                    found["memory"] = memory;
                }
                found
            })
            .collect();
        Ok(ActionOutcome::Went(json!({ "query": spec.query, "hits": hits })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A MEMORY FOUND SAYS ITS TREE**, by the short name a person calls it,
    /// beside its label; one that holds in every tree says no tree; a hit that
    /// is no memory says nothing of the kind.
    #[test]
    fn the_ledger_search_action_names_the_tree_of_a_memory_it_finds() {
        let dir = std::env::temp_dir().join(format!("sailor-search-memory-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        let memory = |label: &str, value: &str, tree: Option<&str>| Memory {
            kind: "project".to_owned(),
            label: label.to_owned(),
            value: value.to_owned(),
            provenance: "test".to_owned(),
            modified: 1,
            valid_from: 1,
            valid_until: None,
            tree: tree.map(str::to_owned),
        };
        crate::memory::remember(&ledger, memory("the trunk", "a quokka sits on sorgenti", Some("/trees/a-checkout")))
            .expect("kept");
        crate::memory::remember(&ledger, memory("the home", "a quokka sits under state", None)).expect("kept");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: "notes".to_owned(),
                key: "one".to_owned(),
                value: json!({ "text": "a quokka in the notes" }),
                written_by: "test".to_owned(),
                written_at: 3,
            })
            .expect("a record");
        let went = LedgerSearchAction::new(Some(ledger.clone()), None)
            .execute(&json!({"query": "quokka"}), &SharedState::default())
            .expect("searched");
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        let ActionOutcome::Went(output) = went else { panic!("the search went: {went:?}") };
        let hits = output["hits"].as_array().expect("hits");
        let by_id = |id: &str| hits.iter().find(|hit| hit["id"] == id).unwrap_or_else(|| panic!("{id} among {hits:?}"));
        assert_eq!(by_id("store:memories/the-trunk")["memory"], json!({"label": "the trunk", "tree": "a-checkout"}));
        assert_eq!(by_id("store:memories/the-home")["memory"], json!({"label": "the home", "tree": null}));
        assert!(by_id("store:notes/one").get("memory").is_none(), "a note is called a memory: {hits:?}");
    }

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

    /// One ranking answers for both stores: a store entry of the ledger and a
    /// fault of the register that say the same word are both in it.
    #[test]
    fn the_ledger_and_the_fault_register_answer_in_one_ranking() {
        let dir = std::env::temp_dir().join(format!("sailor-search-union-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: "notes".to_owned(),
                key: "one".to_owned(),
                value: json!({ "text": "a quokka in the store" }),
                written_by: "test".to_owned(),
                written_at: 3,
            })
            .expect("a record");
        let faults_store = dir.join(faults::FAULTS_FILE);
        let fault = Faults::open(&faults_store)
            .expect("a register")
            .record(&faults::Draft {
                happened_on: "01/01/2000".to_owned(),
                what_happened: "a quokka in the register".to_owned(),
                how_it_showed: "in a test".to_owned(),
                what_would_prevent: "a door".to_owned(),
                status: "**aperto**".to_owned(),
            })
            .expect("a fault");
        let ids: Vec<String> = search_the_ledger_and_the_faults(&ledger, Some(&faults_store), "quokka")
            .expect("a ranking")
            .into_iter()
            .map(|hit| hit.id)
            .collect();
        drop(ledger);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(ids.contains(&"store:notes/one".to_owned()), "{ids:?}");
        assert!(ids.contains(&format!("fault:{}", fault.number)), "{ids:?}");
    }
}
