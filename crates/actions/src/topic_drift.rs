//! Whether a mandate still belongs to the workspace it arrived in, read from
//! the same graph [`graph_memory`] already keeps — literal keyword overlap,
//! the same paradigm `sailor search` already uses, no embedding involved.
//!
//! **THIS ACTION NEVER MOVES ANYTHING.** It only answers whether a match
//! elsewhere beats a match at home; the step that reads it decides what to propose.

use crate::graph_memory::NODES_COLLECTION;
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::Ledger;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The name [`TopicDriftAction`] registers itself under.
pub const TOPIC_DRIFT_ACTION: &str = "topic_drift";

pub fn register_topic_drift(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(TOPIC_DRIFT_ACTION, TopicDriftAction::new(ledger));
}

/// Lowercase words of at least three letters, deduplicated. Long enough to
/// throw away most stray punctuation and connective words without keeping a
/// stop-word list — a list is one more thing to keep in step with a language
/// the tree may change.
fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|word| word.to_lowercase())
        .filter(|word| word.chars().count() >= 3)
        .collect()
}

/// The intersection over the union of two word sets — `0.0` when neither
/// shares nor has anything, so an empty graph never reads as a perfect
/// match.
fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}

#[derive(Debug, Deserialize)]
struct DriftSpec {
    workspace_id: String,
    text: String,
}

pub struct TopicDriftAction {
    ledger: Option<Ledger>,
}

impl TopicDriftAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }

    /// One vocabulary per workspace still holding current nodes: every
    /// superseded revision is left out, the same rule a query already
    /// applies, so a retired decision cannot anchor a workspace it no
    /// longer describes.
    fn vocabularies(ledger: &Ledger) -> Result<BTreeMap<String, BTreeSet<String>>, ActionError> {
        let nodes = ledger
            .records_in(NODES_COLLECTION)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let mut by_workspace: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for record in nodes {
            if !record.value["superseded_by"].is_null() {
                continue;
            }
            let Some(workspace_id) = record.value["workspace_id"].as_str() else {
                continue;
            };
            let vocabulary = by_workspace.entry(workspace_id.to_owned()).or_default();
            if let Some(title) = record.value["title"].as_str() {
                vocabulary.extend(words(title));
            }
            if let Some(summary) = record.value["summary"].as_str() {
                vocabulary.extend(words(summary));
            }
        }
        Ok(by_workspace)
    }
}

impl Action for TopicDriftAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: DriftSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let ledger = self.ledger.as_ref().ok_or_else(|| {
            ActionError::new(
                "no_store",
                "I cannot read the memory graph, so I cannot tell a divergence from silence"
                    .to_owned(),
            )
        })?;

        let by_workspace = Self::vocabularies(ledger)?;
        let text_words = words(&spec.text);
        let own_overlap = by_workspace
            .get(&spec.workspace_id)
            .map(|vocabulary| jaccard(&text_words, vocabulary))
            .unwrap_or(0.0);

        let elsewhere = by_workspace
            .iter()
            .filter(|(workspace_id, _)| **workspace_id != spec.workspace_id)
            .map(|(workspace_id, vocabulary)| (workspace_id.clone(), jaccard(&text_words, vocabulary)))
            .filter(|(_, overlap)| *overlap > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1));

        let (diverges, reason) = match &elsewhere {
            Some((_, overlap)) if *overlap > own_overlap => (true, "matches_elsewhere"),
            _ if own_overlap > 0.0 => (false, "on_topic"),
            _ => (false, "no_signal_anywhere"),
        };

        Ok(ActionOutcome::Went(json!({
            "workspace_id": spec.workspace_id,
            "own_overlap": own_overlap,
            "elsewhere": elsewhere.map(|(workspace_id, overlap)| json!({
                "workspace_id": workspace_id,
                "overlap": overlap,
            })),
            "diverges": diverges,
            "reason": reason,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_memory::MemoryWriteAction;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestStore(std::path::PathBuf);

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store() -> (Ledger, TestStore) {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-actions-topic-drift-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("open the ledger");
        (ledger, TestStore(path))
    }

    fn node(workspace_id: &str, node_id: &str, title: &str, summary: &str) -> Value {
        json!({
            "what": "node",
            "workspace_id": workspace_id,
            "node_id": node_id,
            "kind": "note",
            "title": title,
            "summary": summary,
            "written_by": "test",
        })
    }

    fn drift(ledger: &Ledger, workspace_id: &str, text: &str) -> Value {
        let action = TopicDriftAction::new(Some(ledger.clone()));
        let ActionOutcome::Went(answer) = action
            .execute(&json!({"workspace_id": workspace_id, "text": text}), &SharedState::new())
            .expect("the reading")
        else {
            panic!("a node touching a local ledger waits for nobody");
        };
        answer
    }

    /// Text that shares nothing with any workspace's graph is not a
    /// divergence: it is a workspace with nothing written down yet, and
    /// silence is not evidence of another topic.
    #[test]
    fn silence_everywhere_is_not_a_divergence() {
        let (ledger, _guard) = store();
        let answer = drift(&ledger, "acme-products", "let's talk about the weather today");
        assert_eq!(answer["diverges"], json!(false), "{answer}");
        assert_eq!(answer["reason"], json!("no_signal_anywhere"), "{answer}");
    }

    /// Text that matches its own workspace's graph, and nowhere else better,
    /// stays on topic.
    #[test]
    fn matching_ones_own_graph_is_on_topic() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(ledger.clone());
        write
            .execute(
                &node("acme-products", "pricing", "pricing model", "flat fee decision"),
                &SharedState::new(),
            )
            .expect("write the node");

        let answer = drift(&ledger, "acme-products", "let's revisit the pricing model");
        assert_eq!(answer["diverges"], json!(false), "{answer}");
        assert_eq!(answer["reason"], json!("on_topic"), "{answer}");
    }

    /// A stray shared word with another workspace is not enough: the
    /// divergence has to *win*, not merely exist, or a single common word
    /// like a product's own name would false-flag every mandate.
    #[test]
    fn a_weaker_match_elsewhere_does_not_override_a_stronger_match_at_home() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(ledger.clone());
        write
            .execute(
                &node("acme-products", "pricing", "acme pricing model", "flat fee decision for acme"),
                &SharedState::new(),
            )
            .expect("products");
        write
            .execute(&node("acme-ads", "budget", "acme campaign budget", "spend"), &SharedState::new())
            .expect("ads");

        let answer = drift(&ledger, "acme-products", "let's revisit the acme pricing model decision");
        assert_eq!(answer["diverges"], json!(false), "{answer}");
        assert_eq!(answer["reason"], json!("on_topic"), "{answer}");
    }

    /// **THE CASE THIS ACTION EXISTS FOR**, and it is the owner's own example: a
    /// session in
    /// `acme-products` starts talking like `acme-ads` instead.
    #[test]
    fn matching_another_workspace_better_is_a_divergence() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(ledger.clone());
        write
            .execute(&node("acme-products", "pricing", "pricing model", "flat fee decision"), &SharedState::new())
            .expect("products");
        write
            .execute(&node("acme-ads", "budget", "campaign budget", "ten k per month on ads"), &SharedState::new())
            .expect("ads");

        let answer = drift(&ledger, "acme-products", "what should the campaign budget be this month");
        assert_eq!(answer["diverges"], json!(true), "{answer}");
        assert_eq!(answer["reason"], json!("matches_elsewhere"), "{answer}");
        assert_eq!(answer["elsewhere"]["workspace_id"], json!("acme-ads"), "{answer}");
    }

    /// A superseded node's words do not anchor its workspace any more: once
    /// `graph_memory` marks a revision replaced, this reading must not still
    /// count it as evidence of what the workspace is about.
    #[test]
    fn a_superseded_node_does_not_count_toward_its_workspace() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(ledger.clone());
        write
            .execute(&node("acme-products", "pricing", "pricing model", "flat fee decision"), &SharedState::new())
            .expect("first revision");
        write
            .execute(&node("acme-products", "pricing", "usage based billing", "per call cost"), &SharedState::new())
            .expect("second revision supersedes the first");

        let answer = drift(&ledger, "acme-products", "let's revisit the flat fee decision");
        assert_eq!(answer["reason"], json!("no_signal_anywhere"), "{answer}");
    }

    /// **BLIND IS NOT EMPTY**, the same rule every other reading of this
    /// graph already holds.
    #[test]
    fn without_a_store_it_refuses_instead_of_answering_on_topic() {
        let action = TopicDriftAction::new(None);
        let error = action
            .execute(&json!({"workspace_id": "acme-products", "text": "anything"}), &SharedState::new())
            .expect_err("no store to read from");
        assert_eq!(error.class, "no_store");
    }
}
