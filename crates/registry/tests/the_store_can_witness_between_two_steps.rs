//! The store bears witness between two steps: **with the real registry, the
//! real store and the real executor**, no fake in between.
//!
//! **WHY IT EXISTS — THE SYMPTOM OF FAULT 28, MADE A FACT AT ZERO COST.** A
//! step writing into the store a key taken from the step before died with
//! "invalid type: map, expected a string": `store_write` did not resolve
//! references, so it received `{"$from": "/stdout"}` as an object. The store
//! accepted only values written by hand inside the flow, so it could not be
//! the witness `docs/decisions.md` credits it with.
//!
//! **WHY HERE AND NOT IN `crates/actions`.** The proof that lived there called
//! `execute` by hand: now that `flow::step_input` resolves the references, such
//! a proof would have to resolve them itself before calling — that is, prove
//! itself. Here instead all three pieces that were in play pass through: the
//! registry that assembles the actions (`default_registry`), the executor that
//! composes the input, and SQLite receiving the key.
//!
//! **NOTHING IS SPENT AND NO MODEL IS CALLED**: the first step is
//! `sh -c printf`, an engine that answers with no providers.
//!
//! **THE MUTANT**: take `resolve_references` out of `step_input` — the original
//! defect. The run does not reach the end and the depositing step breaks with
//! `invalid_input`, the same words as the fault.

use flow::{
    ExecutionRequest, Executor, Graph, InProcessExecutor, Outcome, RecordStore, SharedState, Step,
    SystemClock, ValueSchema,
};
use ledger::Ledger;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// A throwaway directory for each proof.
///
/// **THE COUNTER IN THE NAME IS NOT ORNAMENT**: it is fault 21. `cargo test`
/// runs the proofs in one process and the macOS clock has no nanosecond
/// resolution.
struct TestDirectory(PathBuf);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-store-witness-{label}-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("creare la cartella del deposito di prova");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn step(id: &str, deps: &[&str], action: &str, with: Value) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        action: action.to_owned(),
        max_attempts: 1,
        ask_again_after_secs: None,
        retry_after_secs: None,
        phase: None,
        stops_when: None,
        decides_done: false,
        when: None,
        with: Some(with),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::Any,
    }
}

/// **THE KEY COMES FROM THE STEP BEFORE, AND THE STORE RECEIVES IT AS TEXT.**
///
/// Three steps: an engine answers, the store writes under the key that engine
/// named, and a third step reads the entry back naming the key by hand. Were
/// the reference not resolved, the second step would break and the third find
/// nothing — which is exactly how the fault went.
#[test]
fn a_key_decided_by_the_step_before_reaches_the_real_store() {
    let dir = TestDirectory::new("chiave-dal-passo-prima");
    let ledger = Ledger::open(dir.0.join("deposito")).expect("aprire il deposito");
    let actions = registry::registry_in(registry::House::under(&dir.0), Some(ledger.clone()), None);

    let graph = Graph::new(vec![
        step(
            "chiedi",
            &[],
            "external_engine",
            json!({"bin": "sh", "args": ["-c", "printf 'il-lavoro-di-ieri'"], "timeout_secs": 10}),
        ),
        step(
            "deposita",
            &["chiedi"],
            "store_write",
            json!({
                "collection": "mandato",
                "key": {"$from": "/stdout"},
                "value": {"deciso_da": {"$from": "/stdout"}},
                "written_by": "prova-del-testimone",
                "written_at": 1_756_400_000i64,
            }),
        ),
        step(
            "rileggi",
            &["deposita"],
            "store_read",
            json!({"collection": "mandato", "key": "il-lavoro-di-ieri"}),
        ),
    ])
    .expect("grafo valido");

    let store = ledger.clone();
    InProcessExecutor
        .execute(
            &graph,
            ExecutionRequest {
                run_id: "corsa-del-testimone".to_owned(),
                root_inputs: BTreeMap::new(),
                gates: vec![],
                shared: SharedState::new(),
                spend_cap_micros: None,
                stops: flow::RunStops::default(),
            },
            &store,
            &actions,
            &SystemClock,
        )
        .expect("la corsa arriva in fondo");

    let records = store
        .records("corsa-del-testimone")
        .expect("rileggere i record della corsa");
    let closed: BTreeMap<String, Outcome> = records
        .iter()
        .filter_map(|record| {
            record
                .outcome
                .map(|outcome| (record.step_id.clone(), outcome))
        })
        .collect();
    assert_eq!(
        closed.get("deposita"),
        Some(&Outcome::Went),
        "il passo che deposita non è andato: {:?}",
        records
            .iter()
            .filter(|record| record.step_id == "deposita")
            .map(|record| record.said.clone())
            .collect::<Vec<_>>()
    );

    let read = records
        .iter()
        .find(|record| record.step_id == "rileggi" && record.outcome == Some(Outcome::Went))
        .and_then(|record| record.output.clone())
        .expect("il passo che rilegge ha un'uscita");
    assert_eq!(
        read["found"],
        json!(true),
        "la voce non è stata trovata sotto la chiave che il motore ha detto: {read}"
    );
    assert_eq!(read["value"]["deciso_da"], json!("il-lavoro-di-ieri"));
    assert_eq!(read["written_by"], json!("prova-del-testimone"));
}
