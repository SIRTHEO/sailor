//! The one place a flow deposits what it learned, and the one place another
//! flow asks for it — nodes and typed edges on the existing `store` table,
//! not a ninth file in `docs/`. Scoped by workspace so several areas of one
//! project stay apart; an edge crosses that boundary only when a step
//! declares one, never inferred. Writing the same `node_id` again supersedes
//! the old revision instead of erasing it.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// The name `MemoryWriteAction` registers itself under.
pub const MEMORY_WRITE_ACTION: &str = "memory_write";
/// The name `MemoryQueryAction` registers itself under.
pub const MEMORY_QUERY_ACTION: &str = "memory_query";

/// The ledger collection nodes live in.
pub const NODES_COLLECTION: &str = "graph-nodes";
/// The ledger collection edges live in.
pub const EDGES_COLLECTION: &str = "graph-edges";

/// **THE QUERY REGISTERS WITHOUT A STORE, THE WRITE DOES NOT.** The same
/// choice `presence` already made, for the same reason: a flow naming
/// `memory_query` must be sayable by `flow check` without opening anything,
/// and asking it a real question without a store must refuse, not answer
/// «nothing is known».
pub fn register_graph_memory(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(MEMORY_QUERY_ACTION, MemoryQueryAction::new(ledger.clone()));
    registry.register(MEMORY_WRITE_ACTION, MemoryWriteAction::new(ledger));
}

/// The store, or the refusal saying what cannot be done without one.
fn deposit<'a>(ledger: &'a Option<Ledger>, without: &str) -> Result<&'a Ledger, ActionError> {
    ledger.as_ref().ok_or_else(|| {
        ActionError::new(
            "no_store",
            format!("I cannot tell where the store lives, so {without}"),
        )
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn node_key(workspace_id: &str, node_id: &str, revision: i64) -> String {
    format!("{workspace_id}:{node_id}:{revision}")
}

fn edge_key(workspace_id: &str, from: &str, to: &str, kind: &str) -> String {
    format!("{workspace_id}:{from}->{to}:{kind}")
}

#[derive(Debug, Deserialize)]
struct NodeSpec {
    workspace_id: String,
    node_id: String,
    kind: String,
    title: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    status: Option<String>,
    written_by: String,
    #[serde(default)]
    at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EdgeSpec {
    /// The edge's own scope. **NOT NECESSARILY `from`'s OR `to`'s workspace**:
    /// the point of an edge is to declare a link *between* areas, and one of
    /// the two ends is routinely in another workspace on purpose.
    workspace_id: String,
    from: String,
    to: String,
    kind: String,
    written_by: String,
    #[serde(default)]
    at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "what", rename_all = "snake_case")]
enum WriteSpec {
    Node(NodeSpec),
    Edge(EdgeSpec),
}

pub struct MemoryWriteAction {
    ledger: Option<Ledger>,
}

impl MemoryWriteAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }

    /// **THIS READ-THEN-WRITE IS NOT ITSELF ATOMIC.** Two callers racing the
    /// same `{workspace_id}:{node_id}` without a claim could compute the same
    /// `revision` and one write would silently replace the other's key. The
    /// safety today is `remember-in-the-graph`'s, whose `claim` step runs
    /// before this one — a caller that skips `work_claim` first accepts this
    /// race; this action does not check for one itself.
    fn write_node(&self, spec: NodeSpec) -> Result<ActionOutcome, ActionError> {
        let at = spec.at.unwrap_or_else(now);
        let ledger = deposit(&self.ledger, "the node would be remembered nowhere")?;
        let existing = ledger
            .records_in(NODES_COLLECTION)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        // The current revision of this node in this workspace, if it has one:
        // the one no later revision has named as what replaced it.
        let current = existing.iter().find(|record| {
            record.value["workspace_id"] == json!(spec.workspace_id)
                && record.value["node_id"] == json!(spec.node_id)
                && record.value["superseded_by"].is_null()
        });
        let revision = current.and_then(|record| record.value["revision"].as_i64()).unwrap_or(0) + 1;
        let new_key = node_key(&spec.workspace_id, &spec.node_id, revision);

        if let Some(previous) = current {
            let mut superseded_value = previous.value.clone();
            superseded_value["superseded_by"] = json!(new_key);
            ledger
                .put_record(&StoreRecord {
                    collection: NODES_COLLECTION.to_owned(),
                    key: previous.key.clone(),
                    value: superseded_value,
                    written_by: previous.written_by.clone(),
                    written_at: previous.written_at,
                })
                .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        }

        ledger
            .put_record(&StoreRecord {
                collection: NODES_COLLECTION.to_owned(),
                key: new_key.clone(),
                value: json!({
                    "workspace_id": spec.workspace_id,
                    "node_id": spec.node_id,
                    "revision": revision,
                    "kind": spec.kind,
                    "title": spec.title,
                    "summary": spec.summary,
                    "status": spec.status.unwrap_or_else(|| "open".to_owned()),
                    "superseded_by": Value::Null,
                }),
                written_by: spec.written_by,
                written_at: at,
            })
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;

        Ok(ActionOutcome::Went(json!({
            "key": new_key,
            "revision": revision,
            "superseded": current.map(|previous| previous.key.clone()),
        })))
    }

    fn write_edge(&self, spec: EdgeSpec) -> Result<ActionOutcome, ActionError> {
        let at = spec.at.unwrap_or_else(now);
        let key = edge_key(&spec.workspace_id, &spec.from, &spec.to, &spec.kind);
        let ledger = deposit(&self.ledger, "the edge would be remembered nowhere")?;
        ledger
            .put_record(&StoreRecord {
                collection: EDGES_COLLECTION.to_owned(),
                key: key.clone(),
                value: json!({
                    "workspace_id": spec.workspace_id,
                    "from": spec.from,
                    "to": spec.to,
                    "kind": spec.kind,
                }),
                written_by: spec.written_by,
                written_at: at,
            })
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({ "key": key })))
    }
}

impl Action for MemoryWriteAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: WriteSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        match spec {
            WriteSpec::Node(node) => self.write_node(node),
            WriteSpec::Edge(edge) => self.write_edge(edge),
        }
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[derive(Debug, Deserialize)]
struct QuerySpec {
    workspace_id: String,
    #[serde(default)]
    kind: Option<String>,
    /// Asking about one node also returns the edges that touch it, **on
    /// either end** — including the ones reaching into another workspace,
    /// which is the whole point of declaring one.
    #[serde(default)]
    node_id: Option<String>,
}

pub struct MemoryQueryAction {
    ledger: Option<Ledger>,
}

impl MemoryQueryAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for MemoryQueryAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: QuerySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let ledger = self.ledger.as_ref().ok_or_else(|| {
            ActionError::new(
                "no_store",
                "I cannot read the memory graph, and an empty answer here would say «nothing is \
                 known» over a workspace that may hold plenty"
                    .to_owned(),
            )
        })?;

        let nodes = ledger
            .records_in(NODES_COLLECTION)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let mut matched: Vec<Value> = nodes
            .into_iter()
            .filter(|record| record.value["workspace_id"] == json!(spec.workspace_id))
            .filter(|record| record.value["superseded_by"].is_null())
            .filter(|record| spec.kind.as_deref().is_none_or(|kind| record.value["kind"] == json!(kind)))
            .filter(|record| {
                spec.node_id
                    .as_deref()
                    .is_none_or(|node_id| record.value["node_id"] == json!(node_id))
            })
            .map(|record| record.value)
            .collect();
        matched.sort_by(|a, b| a["node_id"].as_str().cmp(&b["node_id"].as_str()));

        let mut edges: Vec<Value> = Vec::new();
        if let Some(node_id) = &spec.node_id {
            let all_edges = ledger
                .records_in(EDGES_COLLECTION)
                .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
            edges = all_edges
                .into_iter()
                .filter(|record| {
                    record.value["from"] == json!(node_id) || record.value["to"] == json!(node_id)
                })
                .map(|record| record.value)
                .collect();
        }

        Ok(ActionOutcome::Went(json!({
            "workspace_id": spec.workspace_id,
            "nodes": matched,
            "edges": edges,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "sailor-actions-graph-memory-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("open the ledger");
        (ledger, TestStore(path))
    }

    fn went(outcome: ActionOutcome) -> Value {
        let ActionOutcome::Went(value) = outcome else {
            panic!("a node touching a local ledger waits for nobody");
        };
        value
    }

    fn node(workspace_id: &str, node_id: &str, kind: &str, title: &str) -> Value {
        json!({
            "what": "node",
            "workspace_id": workspace_id,
            "node_id": node_id,
            "kind": kind,
            "title": title,
            "written_by": "test",
        })
    }

    /// The first write of a node is revision one, superseding nothing.
    #[test]
    fn the_first_write_of_a_node_is_revision_one() {
        let (ledger, _guard) = store();
        let action = MemoryWriteAction::new(Some(ledger));
        let shared = SharedState::new();

        let answer = went(
            action
                .execute(&node("acme-products", "pricing-model", "decision", "Flat fee"), &shared)
                .expect("the write"),
        );
        assert_eq!(answer["revision"], json!(1));
        assert_eq!(answer["superseded"], Value::Null);
    }

    /// **A NODE IS SUPERSEDED, NOT OVERWRITTEN.** Writing `pricing-model` again
    /// leaves the first revision readable, marked with what replaced it — a
    /// query for current nodes sees only the second, never both, never neither.
    #[test]
    fn writing_a_node_again_supersedes_the_old_revision_instead_of_erasing_it() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(Some(ledger.clone()));
        let query = MemoryQueryAction::new(Some(ledger));
        let shared = SharedState::new();

        let first = went(
            write
                .execute(&node("acme-products", "pricing-model", "decision", "Flat fee"), &shared)
                .expect("the first write"),
        );
        let second = went(
            write
                .execute(
                    &node("acme-products", "pricing-model", "decision", "Usage-based"),
                    &shared,
                )
                .expect("the second write"),
        );
        assert_eq!(second["revision"], json!(2));
        assert_eq!(second["superseded"], first["key"]);

        let seen = went(
            query
                .execute(&json!({"workspace_id": "acme-products"}), &shared)
                .expect("the query"),
        );
        let nodes = seen["nodes"].as_array().expect("the nodes");
        assert_eq!(nodes.len(), 1, "a superseded revision must not also count as current: {seen}");
        assert_eq!(nodes[0]["title"], json!("Usage-based"));
    }

    /// **THE BOUNDARY IS THE FIELD, NOT AN ASSUMPTION.** Two areas of one
    /// project — products and ads, the owner's own example — stay apart by default.
    #[test]
    fn a_query_never_crosses_into_another_workspace_by_itself() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(Some(ledger.clone()));
        let query = MemoryQueryAction::new(Some(ledger));
        let shared = SharedState::new();

        write
            .execute(&node("acme-products", "launch-date", "decision", "Q1"), &shared)
            .expect("products");
        write
            .execute(&node("acme-ads", "budget", "decision", "10k/month"), &shared)
            .expect("ads");

        let seen = went(
            query
                .execute(&json!({"workspace_id": "acme-products"}), &shared)
                .expect("the query"),
        );
        let nodes = seen["nodes"].as_array().expect("the nodes");
        assert_eq!(nodes.len(), 1, "{seen}");
        assert_eq!(nodes[0]["node_id"], json!("launch-date"));
    }

    /// A declared edge crosses the boundary the query does not: asking about a
    /// node returns the edges that name it, on either side, wherever they lead.
    #[test]
    fn asking_about_a_node_returns_the_edges_that_reach_it_even_across_workspaces() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(Some(ledger.clone()));
        let query = MemoryQueryAction::new(Some(ledger));
        let shared = SharedState::new();

        write
            .execute(&node("acme-products", "launch-date", "decision", "Q1"), &shared)
            .expect("products");
        write
            .execute(&node("acme-ads", "campaign-start", "proposal", "Align to launch"), &shared)
            .expect("ads");
        write
            .execute(
                &json!({
                    "what": "edge",
                    "workspace_id": "acme-ads",
                    "from": "campaign-start",
                    "to": "launch-date",
                    "kind": "depends_on",
                    "written_by": "test",
                }),
                &shared,
            )
            .expect("the edge");

        let seen = went(
            query
                .execute(&json!({"workspace_id": "acme-products", "node_id": "launch-date"}), &shared)
                .expect("the query"),
        );
        let edges = seen["edges"].as_array().expect("the edges");
        assert_eq!(edges.len(), 1, "{seen}");
        assert_eq!(edges[0]["from"], json!("campaign-start"));
    }

    /// **BLIND IS NOT EMPTY.** A query with no store to read must refuse, the
    /// same choice `work_survey` already made: an empty answer here would be
    /// read as «this workspace holds nothing», which may simply be false.
    #[test]
    fn a_query_with_no_store_refuses_instead_of_answering_nothing_is_known() {
        let query = MemoryQueryAction::new(None);
        let error = query
            .execute(&json!({"workspace_id": "acme-products"}), &SharedState::new())
            .expect_err("no store to read from");
        assert_eq!(error.class, "no_store");
    }

    /// The `kind` filter narrows what a query returns.
    #[test]
    fn a_kind_filter_narrows_the_nodes_returned() {
        let (ledger, _guard) = store();
        let write = MemoryWriteAction::new(Some(ledger.clone()));
        let query = MemoryQueryAction::new(Some(ledger));
        let shared = SharedState::new();

        write
            .execute(&node("acme-products", "launch-date", "decision", "Q1"), &shared)
            .expect("a decision");
        write
            .execute(&node("acme-products", "faster-checkout", "proposal", "Skip a step"), &shared)
            .expect("a proposal");

        let seen = went(
            query
                .execute(&json!({"workspace_id": "acme-products", "kind": "proposal"}), &shared)
                .expect("the query"),
        );
        let nodes = seen["nodes"].as_array().expect("the nodes");
        assert_eq!(nodes.len(), 1, "{seen}");
        assert_eq!(nodes[0]["node_id"], json!("faster-checkout"));
    }
}
