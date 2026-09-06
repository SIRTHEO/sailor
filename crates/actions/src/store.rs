//! The two nodes a flow remembers something with, from one run to the next.
//!
//! **WHY THEY EXIST.** The engine runs a graph, but it could not *remember*
//! anything that was not a run, a step or a call to a model. Every fact a job
//! had to keep — which work it is on, when it last ran, what it had already
//! seen — ended up in a Rust struct written for the purpose. That is how
//! `notte` grew to 2,562 lines for a four-step flow, and why it is condemned.
//!
//! Here the fact lives in a **collection** named by whoever writes the flow,
//! and these nodes are the only ones that touch it. The engine does not know
//! what that name means, and does not need to: it knows how to keep it.
//!
//! **The store arrives at registration, not at execution.** An action receives
//! only its own input and the shared state — the contract that keeps `flow`
//! agnostic of any service — so whoever registers these nodes hands them the
//! store to work on. A flow cannot pick a store other than its runner's: the
//! namespace is the flow's, the file is not.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StoreRecord};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

/// The name `StoreWriteAction` registers under.
pub const STORE_WRITE_ACTION: &str = "store_write";
/// The name `StoreReadAction` registers under.
pub const STORE_READ_ACTION: &str = "store_read";
/// The name `StoreListAction` registers under.
pub const STORE_LIST_ACTION: &str = "store_list";

/// Registers the three store nodes, **store or no store**: `flow check` must be
/// able to say the step names a real action without opening anything, and
/// without a store the run refuses instead of pretending.
pub fn register_store(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(STORE_WRITE_ACTION, StoreWriteAction::new(ledger.clone()));
    registry.register(STORE_READ_ACTION, StoreReadAction::new(ledger.clone()));
    registry.register(STORE_LIST_ACTION, StoreListAction::new(ledger));
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

#[derive(Debug, Deserialize)]
struct WriteSpec {
    collection: String,
    /// Where the entry goes. **Absent means "this run"**: the key becomes the
    /// run's own id, which the flow cannot name — it reaches no step's input
    /// and is born after the file. A fixed key written here would overwrite
    /// the previous run's entry on every turn.
    #[serde(default)]
    key: Option<String>,
    value: Value,
    /// Who is writing it. The flow declares it so whoever reads the entry back
    /// knows where it came from: an entry with no author can be read, but not
    /// contested.
    written_by: String,
    /// The instant, when the flow dictates it. The tests need it — they would
    /// otherwise hang on the clock — and so does anyone restating a dated fact.
    #[serde(default)]
    written_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ReadSpec {
    collection: String,
    key: String,
}

/// The run's id, for an entry that declares no key of its own.
///
/// Read from shared state and not from the step's input: offering it to every
/// step is the shape that already killed a shipped flow on `unknown field`,
/// because a closed action spec refuses what the executor adds. Whoever needs
/// it reads it here, as the node that hands work to a person already does.
fn key_of_this_run(shared: &SharedState) -> Result<String, ActionError> {
    shared
        .get(flow::CURRENT_RUN)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            ActionError::new(
                "no_run",
                "the entry names no key and there is no run to name it after: writing it \
                 under a made-up key would overwrite somebody else's",
            )
        })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Writes an entry into the collection the flow named.
///
/// **Repeatable by construction**: an entry is identified by collection and
/// key, so writing the same value again leaves the store as it was. That is
/// why this node can be relaunched without handing anything to a person —
/// unlike a node that sends a line to a terminal, which the world cannot undo.
pub struct StoreWriteAction {
    ledger: Option<Ledger>,
}

impl StoreWriteAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for StoreWriteAction {
    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // The input arrives with its references already resolved: `step_input`
        // resolves them where the input is composed, for every action and once
        // over. That is how what a step produced reaches the store without
        // leaving the graph — and without it, this node would take values
        // hand-written in the flow, and could not witness between two steps.
        let spec: WriteSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let key = match spec.key {
            Some(key) => key,
            None => key_of_this_run(shared)?,
        };
        let ledger = deposit(&self.ledger, "there is nowhere to put this entry")?;
        let record = StoreRecord {
            collection: spec.collection,
            key,
            value: spec.value,
            written_by: spec.written_by,
            written_at: spec.written_at.unwrap_or_else(now),
        };
        // An empty address is a mistake by whoever wrote the flow, not a fact
        // of the world: it is said at once, in the store's own words.
        ledger
            .put_record(&record)
            .map_err(|error| ActionError::new("store_refused", error.to_string()))?;
        Ok(ActionOutcome::Went(json!({
            "collection": record.collection,
            "key": record.key,
            "written_at": record.written_at,
        })))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Reads an entry, and says **whether it was there**.
///
/// An entry nobody has written yet is not a failure of the step: it is the
/// answer. Whoever receives it has a branch for «I do not know» — exactly the
/// case of a job running for the first time, and the case where, before this
/// node existed, somebody would have started guessing.
pub struct StoreReadAction {
    ledger: Option<Ledger>,
}

impl StoreReadAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for StoreReadAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ReadSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let found = deposit(&self.ledger, "«not written yet» would be a guess")?
            .read_record(&spec.collection, &spec.key)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        Ok(ActionOutcome::Went(match found {
            Some(record) => json!({
                "found": true,
                "value": record.value,
                "written_by": record.written_by,
                "written_at": record.written_at,
            }),
            None => json!({ "found": false }),
        }))
    }

    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[derive(Debug, Deserialize)]
struct ListSpec {
    collection: String,
    /// Only the entries whose key sorts **after** this one, as text.
    ///
    /// It is the «from here on» of a reader who has read: without it every turn
    /// rereads the whole collection and the cost grows with the history —
    /// rebuilding in the store the problem the window was meant to shed.
    #[serde(default)]
    after: Option<String>,
}

/// Lists a collection's entries, oldest first.
///
/// **WHAT IT IS FOR, GIVEN `store_read`.** Reading by key presupposes knowing
/// the key: fine for a single fact — which work one is on — and useless for a
/// growing collection, where the question is «what is new». Two different
/// questions, and while just the first existed no flow could keep a list.
///
/// **THE ORDER IS THE KEYS', AND IT IS THE PACT WITH WHOEVER WRITES.** The
/// store sorts by text, not by date: a collection meant to read in time order
/// needs keys sortable as text — an ISO 8601 instant, or a zero-padded number.
/// A badly chosen key gives a list in random order, and nothing flags it.
pub struct StoreListAction {
    ledger: Option<Ledger>,
}

impl StoreListAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }
}

impl Action for StoreListAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: ListSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let records = deposit(&self.ledger, "an empty list would say «nothing is there»")?
            .records_in(&spec.collection)
            .map_err(|error| ActionError::new("store_unreadable", error.to_string()))?;
        let after = spec.after.unwrap_or_default();
        let entries: Vec<Value> = records
            .into_iter()
            .filter(|record| after.is_empty() || record.key > after)
            .map(|record| {
                json!({
                    "key": record.key,
                    "value": record.value,
                    "written_by": record.written_by,
                    "written_at": record.written_at,
                })
            })
            .collect();
        Ok(ActionOutcome::Went(json!({
            "count": entries.len(),
            // The last key, so the next turn knows where to pick up without
            // the flow having to dig it out of the list. Absent when there is
            // nothing new: a reader resuming keeps its own place instead of
            // overwriting it with emptiness.
            "last_key": entries.last().and_then(|e| e.get("key").cloned()),
            "entries": entries,
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
            "sailor-actions-store-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("aprire il deposito");
        (ledger, TestStore(path))
    }

    /// The whole turn, from the node that writes to the node that reads.
    ///
    /// Not a test of two functions: a test that a flow can remember **without
    /// the engine knowing what** — the collection here is called `mandate`,
    /// and that word appears nowhere in the code that runs these two nodes.
    #[test]
    fn a_flow_can_remember_something_the_engine_knows_nothing_about() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let write = StoreWriteAction::new(Some(ledger.clone()));
        let read = StoreReadAction::new(Some(ledger));

        write
            .execute(
                &json!({
                    "collection": "mandate",
                    "key": "current",
                    "value": {"file": "2026-08-28-sailor.md"},
                    "written_by": "flusso-mandato-corrente",
                    "written_at": 1_756_400_000i64,
                }),
                &shared,
            )
            .expect("scrittura");

        let outcome = read
            .execute(
                &json!({"collection": "mandate", "key": "current"}),
                &shared,
            )
            .expect("lettura");
        let ActionOutcome::Went(value) = outcome else {
            panic!("un nodo che legge un deposito locale non aspetta nessuno");
        };
        assert_eq!(value["found"], json!(true));
        assert_eq!(value["value"], json!({"file": "2026-08-28-sailor.md"}));
        assert_eq!(value["written_by"], json!("flusso-mandato-corrente"));
    }

    // **THE SYMPTOM OF FAULT 28 IS TESTED WHERE IT HAPPENS, NOT HERE.** A test
    // written here would resolve the reference by hand before calling — testing
    // the test. The rule lives in `flow::step_input`, is asked of every action
    // in `crates/flow/tests/a_reference_reaches_every_action.rs`, and of the
    // real store in `crates/registry/tests/the_store_can_witness_between_two_steps.rs`,
    // which is where the fault was paid.

    /// An entry never written answers `found: false`, and the step **passes**.
    ///
    /// The mutant that fells it turns the absence into an `ActionError`: the
    /// flow would lose its branch for the first turn, and a new job would be
    /// born red.
    #[test]
    fn a_missing_record_is_an_answer_not_a_failure() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let read = StoreReadAction::new(Some(ledger));

        let outcome = read
            .execute(
                &json!({"collection": "mandate", "key": "current"}),
                &shared,
            )
            .expect("la lettura di una voce assente non è un errore");
        let ActionOutcome::Went(value) = outcome else {
            panic!("nessuna attesa");
        };
        assert_eq!(value["found"], json!(false));
        assert_eq!(
            value.get("value"),
            None,
            "non si inventa un valore che nessuno ha scritto"
        );
    }

    /// A collection reads in order, and «from here on» skips what was read.
    ///
    /// The two assertions prove different things and both are needed: the first
    /// that the order is the keys' — returned any old way, a list of mail would
    /// be useless — the second that `after` really cuts, so a flow that has
    /// read does not pay twice for what it read.
    #[test]
    fn a_collection_reads_in_key_order_and_after_skips_what_was_read() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let write = StoreWriteAction::new(Some(ledger.clone()));
        let list = StoreListAction::new(Some(ledger));

        // Written out of order on purpose: were the order the writing's rather
        // than the key's, the list would come out 03, 01, 02.
        for key in ["2026-08-28T03", "2026-08-28T01", "2026-08-28T02"] {
            write
                .execute(
                    &json!({
                        "collection": "posta/theo",
                        "key": key,
                        "value": {"oggetto": key},
                        "written_by": "prova",
                        "written_at": 1_756_400_000i64,
                    }),
                    &shared,
                )
                .expect("scrittura");
        }

        let ActionOutcome::Went(all) = list
            .execute(&json!({"collection": "posta/theo"}), &shared)
            .expect("elenco")
        else {
            panic!("nessuna attesa");
        };
        assert_eq!(all["count"], json!(3));
        let keys: Vec<&str> = all["entries"]
            .as_array()
            .expect("elenco")
            .iter()
            .map(|e| e["key"].as_str().expect("chiave"))
            .collect();
        assert_eq!(
            keys,
            vec!["2026-08-28T01", "2026-08-28T02", "2026-08-28T03"]
        );
        assert_eq!(all["last_key"], json!("2026-08-28T03"));

        let ActionOutcome::Went(fresh) = list
            .execute(
                &json!({"collection": "posta/theo", "after": "2026-08-28T02"}),
                &shared,
            )
            .expect("elenco")
        else {
            panic!("nessuna attesa");
        };
        assert_eq!(fresh["count"], json!(1), "il già letto non si ripaga");
        assert_eq!(fresh["entries"][0]["key"], json!("2026-08-28T03"));
    }

    /// An empty collection, and one already read to the end.
    ///
    /// `last_key` must be **absent**, not an empty string: a reader resuming
    /// keeps its own place, and an empty `last_key` written over a good one
    /// would make the next turn reread everything.
    #[test]
    fn nothing_new_leaves_the_readers_place_alone() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let list = StoreListAction::new(Some(ledger));

        let ActionOutcome::Went(empty) = list
            .execute(&json!({"collection": "posta/nessuno"}), &shared)
            .expect("elenco")
        else {
            panic!("nessuna attesa");
        };
        assert_eq!(empty["count"], json!(0));
        assert_eq!(empty["last_key"], json!(null));
    }

    /// An empty address is a mistake by whoever wrote the flow.
    ///
    /// The store refuses it, and the node reports the refusal instead of
    /// depositing an entry nobody will ever find again.
    #[test]
    fn an_empty_address_is_refused_with_the_stores_own_words() {
        let (ledger, _guard) = store();
        let shared = SharedState::new();
        let write = StoreWriteAction::new(Some(ledger));

        let error = write
            .execute(
                &json!({
                    "collection": "",
                    "key": "current",
                    "value": 1,
                    "written_by": "prova",
                }),
                &shared,
            )
            .expect_err("una collezione vuota non si scrive");
        assert_eq!(error.class, "store_refused");
    }

    /// **TWO RUNS, TWO ENTRIES.** A flow keeping what it paid for cannot name
    /// its own run, and a fixed key written in the file would overwrite the
    /// entry before it. Two runs and not one: with one, a constant key would
    /// pass this.
    #[test]
    fn an_entry_with_no_key_of_its_own_is_kept_under_the_run() {
        let (ledger, _guard) = store();
        let write = StoreWriteAction::new(Some(ledger.clone()));
        let list = StoreListAction::new(Some(ledger));
        for run in ["corsa-prima", "corsa-seconda"] {
            let mut shared = SharedState::new();
            shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
            let ActionOutcome::Went(written) = write
                .execute(
                    &json!({
                        "collection": "consultations",
                        "value": {"diagnosis": run},
                        "written_by": "prova",
                        "written_at": 1_756_400_000i64,
                    }),
                    &shared,
                )
                .expect("scrittura")
            else {
                panic!("nessuna attesa");
            };
            assert_eq!(written["key"], json!(run));
        }

        let ActionOutcome::Went(all) = list
            .execute(&json!({"collection": "consultations"}), &SharedState::new())
            .expect("elenco")
        else {
            panic!("nessuna attesa");
        };
        assert_eq!(
            all["count"],
            json!(2),
            "the second run wrote over the first"
        );
    }

    /// With no run and no key the node refuses: making one up here would be
    /// choosing which entry of the store to overwrite.
    #[test]
    fn an_entry_with_no_key_and_no_run_is_refused() {
        let (ledger, _guard) = store();
        let error = StoreWriteAction::new(Some(ledger))
            .execute(
                &json!({"collection": "consultations", "value": 1, "written_by": "prova"}),
                &SharedState::new(),
            )
            .expect_err("with no run there is no key to invent");
        assert_eq!(error.class, "no_run");
    }

    /// Without a store the three nodes refuse, and say what they cannot do.
    ///
    /// Silence would be worse: a write into nothing reports success, and a read
    /// with no store answers "not there" to a question nobody could ask.
    #[test]
    fn without_a_store_each_node_refuses_and_says_what_it_cannot_do() {
        let shared = SharedState::new();
        let asked = [
            json!({"collection": "c", "key": "k", "value": 1, "written_by": "prova"}),
            json!({"collection": "c", "key": "k"}),
            json!({"collection": "c"}),
        ];
        let nodes: [Box<dyn Action>; 3] = [
            Box::new(StoreWriteAction::new(None)),
            Box::new(StoreReadAction::new(None)),
            Box::new(StoreListAction::new(None)),
        ];

        for (node, input) in nodes.iter().zip(asked.iter()) {
            let refused = node
                .execute(input, &shared)
                .expect_err("senza deposito non si fa finta");
            assert_eq!(refused.class, "no_store");
            assert!(
                refused.said.contains("where the store lives"),
                "il rifiuto dice perché, non solo che non può: {}",
                refused.said
            );
        }
    }
}
